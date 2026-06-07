use liquid_core::{
    CompleteDatabaseBackup, CreateManagedDatabaseRequest, DatabaseBackupMetadataStore,
    DatabaseBackupStatus, ManagedDatabaseEngine, ManagedDatabaseSslMode, RegisterRequest,
};
use liquid_storage::{LiquidStore, Storage, StorageOptions};

#[tokio::test]
async fn database_backup_store_persists_owner_scoped_jobs_and_restores() {
    let Some(storage) = test_storage().await else {
        return;
    };

    let first = storage
        .register_user(RegisterRequest {
            email: unique_email("backup-first"),
            display_name: "Backup First".to_owned(),
            password: "password123".to_owned(),
        })
        .await
        .unwrap();
    let second = storage
        .register_user(RegisterRequest {
            email: unique_email("backup-second"),
            display_name: "Backup Second".to_owned(),
            password: "password123".to_owned(),
        })
        .await
        .unwrap();
    let source = storage
        .create_managed_database(
            &first.user.id,
            CreateManagedDatabaseRequest {
                name: "Source".to_owned(),
                engine: ManagedDatabaseEngine::Postgres,
                host: "localhost".to_owned(),
                port: 5432,
                database: "source".to_owned(),
                username: "postgres".to_owned(),
                password: "secret123".to_owned(),
                tags: None,
                ssl_mode: ManagedDatabaseSslMode::Disable,
            },
        )
        .await
        .unwrap();
    let target = storage
        .create_managed_database(
            &first.user.id,
            CreateManagedDatabaseRequest {
                name: "Target".to_owned(),
                engine: ManagedDatabaseEngine::Postgres,
                host: "localhost".to_owned(),
                port: 5432,
                database: "target".to_owned(),
                username: "postgres".to_owned(),
                password: "secret123".to_owned(),
                tags: None,
                ssl_mode: ManagedDatabaseSslMode::Disable,
            },
        )
        .await
        .unwrap();

    let backup = storage
        .create_database_backup(
            &first.user.id,
            &source.id,
            Some("before restore".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(backup.status, DatabaseBackupStatus::Queued);
    assert_eq!(backup.source.name, "Source");

    let second_user_backups = storage
        .list_database_backups(&second.user.id, None, None, 10)
        .await
        .unwrap();
    assert!(second_user_backups.is_empty());
    assert!(
        storage
            .get_database_backup(&second.user.id, &backup.id)
            .await
            .is_err()
    );

    let restore_error = storage
        .create_database_restore(
            &first.user.id,
            &backup.id,
            &target.id,
            "restore queued backup".to_owned(),
        )
        .await
        .unwrap_err();
    assert!(restore_error.to_string().contains("succeeded"));

    let claimed = storage
        .claim_next_database_backup("test-worker")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claimed.id, backup.id);
    assert_eq!(claimed.status, DatabaseBackupStatus::Running);

    let completed = storage
        .complete_database_backup(
            &backup.id,
            CompleteDatabaseBackup {
                bucket: "bucket".to_owned(),
                key: "key.dump".to_owned(),
                version_id: Some("version".to_owned()),
                etag: Some("etag".to_owned()),
                size_bytes: 123,
                checksum_sha256: "abc123".to_owned(),
                postgres_server_version: Some("16".to_owned()),
                pg_dump_version: Some("pg_dump 16".to_owned()),
            },
        )
        .await
        .unwrap();
    assert_eq!(completed.status, DatabaseBackupStatus::Succeeded);
    assert_eq!(completed.object.unwrap().key, "key.dump");

    let restore = storage
        .create_database_restore(
            &first.user.id,
            &backup.id,
            &target.id,
            "restore completed backup".to_owned(),
        )
        .await
        .unwrap();
    assert_eq!(restore.status, DatabaseBackupStatus::Queued);
    assert_eq!(restore.target.name, "Target");
}

async fn test_storage() -> Option<Storage> {
    let database_url = std::env::var("LIQUID_TEST_DATABASE_URL").ok()?;
    let storage = Storage::connect_with_options(StorageOptions::new(database_url))
        .await
        .ok()?;
    storage.migrate().await.ok()?;
    Some(storage)
}

fn unique_email(prefix: &str) -> String {
    format!(
        "{prefix}-{}@test.local",
        time::OffsetDateTime::now_utc().unix_timestamp_nanos()
    )
}
