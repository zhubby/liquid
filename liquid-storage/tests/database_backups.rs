use liquid_core::{
    CompleteDatabaseBackup, CreateManagedDatabaseRequest, DatabaseBackupMetadataStore,
    DatabaseBackupStatus, DatabaseBackupStorageKind, ManagedDatabaseEngine, ManagedDatabaseSslMode,
    RegisterRequest,
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
                storage_kind: DatabaseBackupStorageKind::Local,
                local_path: Some("/var/liquid/backups/key.dump".to_owned()),
                bucket: None,
                key: None,
                version_id: None,
                etag: None,
                size_bytes: 123,
                checksum_sha256: "abc123".to_owned(),
                postgres_server_version: Some("16".to_owned()),
                pg_dump_version: Some("pg_dump 16".to_owned()),
            },
        )
        .await
        .unwrap();
    assert_eq!(completed.status, DatabaseBackupStatus::Succeeded);
    let storage_metadata = completed.storage.unwrap();
    assert_eq!(storage_metadata.kind, DatabaseBackupStorageKind::Local);
    assert_eq!(
        storage_metadata.local_path.as_deref(),
        Some("/var/liquid/backups/key.dump")
    );

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

#[tokio::test]
async fn database_backup_store_persists_s3_storage_metadata() {
    let Some(storage) = test_storage().await else {
        return;
    };

    let owner = storage
        .register_user(RegisterRequest {
            email: unique_email("backup-s3"),
            display_name: "Backup S3".to_owned(),
            password: "password123".to_owned(),
        })
        .await
        .unwrap();
    let source = storage
        .create_managed_database(
            &owner.user.id,
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
    let backup = storage
        .create_database_backup(&owner.user.id, &source.id, None)
        .await
        .unwrap();
    storage
        .claim_next_database_backup("test-worker")
        .await
        .unwrap();

    let completed = storage
        .complete_database_backup(
            &backup.id,
            CompleteDatabaseBackup {
                storage_kind: DatabaseBackupStorageKind::S3,
                local_path: None,
                bucket: Some("bucket".to_owned()),
                key: Some("key.dump".to_owned()),
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

    let storage_metadata = completed.storage.unwrap();
    assert_eq!(storage_metadata.kind, DatabaseBackupStorageKind::S3);
    assert_eq!(storage_metadata.bucket.as_deref(), Some("bucket"));
    assert_eq!(storage_metadata.key.as_deref(), Some("key.dump"));
    assert_eq!(storage_metadata.version_id.as_deref(), Some("version"));
    assert_eq!(storage_metadata.etag.as_deref(), Some("etag"));
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
