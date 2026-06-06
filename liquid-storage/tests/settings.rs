use liquid_core::{
    LlmProviderApiMode, LlmProviderKind, RegisterRequest, UpdateLlmProviderSettingsRequest,
};
use liquid_storage::{LiquidStore, Storage, StorageOptions};

#[tokio::test]
async fn llm_provider_settings_upsert_redacts_and_preserves_api_key() {
    let Some(storage) = test_storage().await else {
        return;
    };

    let owner = storage
        .register_user(RegisterRequest {
            email: unique_email("settings-owner"),
            display_name: "Settings Owner".to_owned(),
            password: "password123".to_owned(),
        })
        .await
        .unwrap();

    assert!(
        storage
            .get_llm_provider_settings(&owner.user.id)
            .await
            .unwrap()
            .is_none()
    );

    let created = storage
        .upsert_llm_provider_settings(
            &owner.user.id,
            UpdateLlmProviderSettingsRequest {
                provider: LlmProviderKind::OpenaiCompatible,
                base_url: "https://api.openai.com/v1/chat/completions".to_owned(),
                model: "gpt-4.1".to_owned(),
                api_mode: LlmProviderApiMode::ChatCompletions,
                api_key: Some("sk-test".to_owned()),
            },
        )
        .await
        .unwrap();
    assert_eq!(created.provider, LlmProviderKind::OpenaiCompatible);
    assert_eq!(created.model, "gpt-4.1");
    assert!(created.has_api_key);

    let public_settings = storage
        .get_llm_provider_settings(&owner.user.id)
        .await
        .unwrap()
        .unwrap();
    assert!(public_settings.has_api_key);

    let resolved = storage
        .resolve_llm_provider_settings(&owner.user.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resolved.api_key.as_deref(), Some("sk-test"));

    let updated = storage
        .upsert_llm_provider_settings(
            &owner.user.id,
            UpdateLlmProviderSettingsRequest {
                provider: LlmProviderKind::OpenaiCompatible,
                base_url: "https://llm.example.test/v1/responses".to_owned(),
                model: "gpt-4.1-mini".to_owned(),
                api_mode: LlmProviderApiMode::Responses,
                api_key: Some("   ".to_owned()),
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.model, "gpt-4.1-mini");
    assert!(updated.has_api_key);

    let resolved = storage
        .resolve_llm_provider_settings(&owner.user.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resolved.base_url, "https://llm.example.test/v1/responses");
    assert_eq!(resolved.api_key.as_deref(), Some("sk-test"));
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
