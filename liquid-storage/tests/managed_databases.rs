use liquid_core::{
    CreateManagedDatabaseRequest, ManagedDatabaseEngine, ManagedDatabaseSslMode, RegisterRequest,
};
use liquid_storage::{LiquidStore, Storage, StorageError, StorageOptions};

#[tokio::test]
async fn managed_database_current_selection_persists_and_clears() {
    let Some(storage) = test_storage().await else {
        return;
    };

    let owner = storage
        .register_user(RegisterRequest {
            email: unique_email("managed-database-owner"),
            display_name: "Managed Database Owner".to_owned(),
            password: "password123".to_owned(),
        })
        .await
        .unwrap();
    let other = storage
        .register_user(RegisterRequest {
            email: unique_email("managed-database-other"),
            display_name: "Managed Database Other".to_owned(),
            password: "password123".to_owned(),
        })
        .await
        .unwrap();
    let database = storage
        .create_managed_database(
            &owner.user.id,
            CreateManagedDatabaseRequest {
                name: "Warehouse".to_owned(),
                engine: ManagedDatabaseEngine::Postgres,
                host: "localhost".to_owned(),
                port: 5432,
                database: "warehouse".to_owned(),
                username: "readonly".to_owned(),
                password: "secret123".to_owned(),
                ssl_mode: ManagedDatabaseSslMode::Prefer,
            },
        )
        .await
        .unwrap();

    assert!(
        storage
            .get_current_managed_database(&owner.user.id)
            .await
            .unwrap()
            .is_none()
    );

    let selected = storage
        .set_current_managed_database(&owner.user.id, &database.id)
        .await
        .unwrap();
    assert_eq!(selected.id, database.id);

    let current = storage
        .get_current_managed_database(&owner.user.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(current.name, "Warehouse");

    let isolation_error = storage
        .set_current_managed_database(&other.user.id, &database.id)
        .await
        .unwrap_err();
    assert!(matches!(isolation_error, StorageError::NotFound));

    storage
        .clear_current_managed_database(&owner.user.id)
        .await
        .unwrap();
    assert!(
        storage
            .get_current_managed_database(&owner.user.id)
            .await
            .unwrap()
            .is_none()
    );

    storage
        .set_current_managed_database(&owner.user.id, &database.id)
        .await
        .unwrap();
    storage
        .delete_managed_database(&owner.user.id, &database.id)
        .await
        .unwrap();
    assert!(
        storage
            .get_current_managed_database(&owner.user.id)
            .await
            .unwrap()
            .is_none()
    );
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
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();

    format!("{prefix}-{nanos}@test.local")
}
