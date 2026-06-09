use liquid_core::{
    CompleteDatabaseBackup, CreateManagedDatabaseRequest, DatabaseBackupListFilters,
    DatabaseBackupMetadataStore, DatabaseBackupStatus, DatabaseBackupStorageKind,
    DatabaseBackupTrigger, EnqueueDatabaseBackup, ManagedDatabaseEngine, ManagedDatabaseSslMode,
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

#[tokio::test]
async fn database_backup_store_pages_and_filters_backup_records() {
    let Some(storage) = test_storage().await else {
        return;
    };

    let owner = storage
        .register_user(RegisterRequest {
            email: unique_email("backup-page-owner"),
            display_name: "Backup Page Owner".to_owned(),
            password: "password123".to_owned(),
        })
        .await
        .unwrap();
    let other = storage
        .register_user(RegisterRequest {
            email: unique_email("backup-page-other"),
            display_name: "Backup Page Other".to_owned(),
            password: "password123".to_owned(),
        })
        .await
        .unwrap();
    let first_database = create_database(&storage, &owner.user.id, "Page Source A").await;
    let second_database = create_database(&storage, &owner.user.id, "Page Source B").await;
    let other_database = create_database(&storage, &other.user.id, "Other Source").await;

    let first = storage
        .create_database_backup(&owner.user.id, &first_database.id, Some("first".to_owned()))
        .await
        .unwrap();
    let second = storage
        .create_database_backup(
            &owner.user.id,
            &second_database.id,
            Some("second".to_owned()),
        )
        .await
        .unwrap();
    let cron = storage
        .enqueue_database_backup(
            &owner.user.id,
            EnqueueDatabaseBackup {
                managed_database_id: first_database.id.clone(),
                purpose: Some("cron".to_owned()),
                schedule_id: None,
                trigger: DatabaseBackupTrigger::Cron,
                scheduled_for: None,
                conversation_id: None,
                created_from_turn_id: None,
            },
        )
        .await
        .unwrap();
    storage
        .create_database_backup(&other.user.id, &other_database.id, Some("other".to_owned()))
        .await
        .unwrap();
    let running = storage
        .claim_next_database_backup("paging-worker")
        .await
        .unwrap()
        .unwrap();

    let first_page = storage
        .list_database_backups_page(
            &owner.user.id,
            DatabaseBackupListFilters {
                source_managed_database_id: None,
                status: None,
                trigger: None,
                page: 1,
                page_size: 2,
            },
        )
        .await
        .unwrap();
    assert_eq!(first_page.total_count, 3);
    assert_eq!(first_page.page, 1);
    assert_eq!(first_page.page_size, 2);
    assert_eq!(first_page.records.len(), 2);

    let second_page = storage
        .list_database_backups_page(
            &owner.user.id,
            DatabaseBackupListFilters {
                source_managed_database_id: None,
                status: None,
                trigger: None,
                page: 2,
                page_size: 2,
            },
        )
        .await
        .unwrap();
    assert_eq!(second_page.total_count, 3);
    assert_eq!(second_page.records.len(), 1);

    let first_database_page = storage
        .list_database_backups_page(
            &owner.user.id,
            DatabaseBackupListFilters {
                source_managed_database_id: Some(&first_database.id),
                status: None,
                trigger: None,
                page: 1,
                page_size: 10,
            },
        )
        .await
        .unwrap();
    assert_eq!(first_database_page.total_count, 2);
    assert!(
        first_database_page
            .records
            .iter()
            .all(|record| record.source.id == first_database.id)
    );

    let cron_page = storage
        .list_database_backups_page(
            &owner.user.id,
            DatabaseBackupListFilters {
                source_managed_database_id: None,
                status: None,
                trigger: Some(DatabaseBackupTrigger::Cron),
                page: 1,
                page_size: 10,
            },
        )
        .await
        .unwrap();
    assert_eq!(cron_page.total_count, 1);
    assert_eq!(cron_page.records[0].id, cron.id);

    let running_page = storage
        .list_database_backups_page(
            &owner.user.id,
            DatabaseBackupListFilters {
                source_managed_database_id: None,
                status: Some(DatabaseBackupStatus::Running),
                trigger: None,
                page: 1,
                page_size: 10,
            },
        )
        .await
        .unwrap();
    assert_eq!(running_page.total_count, 1);
    assert_eq!(running_page.records[0].id, running.id);

    let other_owner_page = storage
        .list_database_backups_page(
            &other.user.id,
            DatabaseBackupListFilters {
                source_managed_database_id: None,
                status: None,
                trigger: None,
                page: 1,
                page_size: 10,
            },
        )
        .await
        .unwrap();
    assert_eq!(other_owner_page.total_count, 1);
    assert_ne!(other_owner_page.records[0].id, first.id);
    assert_ne!(other_owner_page.records[0].id, second.id);
}

async fn test_storage() -> Option<Storage> {
    let database_url = std::env::var("LIQUID_TEST_DATABASE_URL").ok()?;
    let storage = Storage::connect_with_options(StorageOptions::new(database_url))
        .await
        .ok()?;
    storage.migrate().await.ok()?;
    Some(storage)
}

async fn create_database(
    storage: &Storage,
    owner_user_id: &str,
    name: &str,
) -> liquid_core::ManagedDatabase {
    storage
        .create_managed_database(
            owner_user_id,
            CreateManagedDatabaseRequest {
                name: name.to_owned(),
                engine: ManagedDatabaseEngine::Postgres,
                host: "localhost".to_owned(),
                port: 5432,
                database: name.to_lowercase().replace(' ', "_"),
                username: "postgres".to_owned(),
                password: "secret123".to_owned(),
                tags: None,
                ssl_mode: ManagedDatabaseSslMode::Disable,
            },
        )
        .await
        .unwrap()
}

fn unique_email(prefix: &str) -> String {
    format!(
        "{prefix}-{}@test.local",
        time::OffsetDateTime::now_utc().unix_timestamp_nanos()
    )
}
