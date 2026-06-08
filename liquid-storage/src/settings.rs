use liquid_core::{
    LlmProviderApiMode, LlmProviderKind, LlmProviderSettings, ResolvedLlmProviderSettings,
    UpdateLlmProviderSettingsRequest,
};

use crate::{
    error::{StorageError, map_database_error},
    store::Storage,
    validation::required_string,
};

pub(crate) async fn get_llm_provider_settings(
    storage: &Storage,
    owner_user_id: &str,
) -> Result<Option<LlmProviderSettings>, StorageError> {
    let row = sqlx::query_as::<_, LlmProviderSettingsRow>(
        r#"
        select provider, base_url, model, api_mode, streaming_enabled,
            encrypted_api_key <> '' as has_api_key
        from user_llm_provider_settings
        where owner_user_id = $1::uuid
        "#,
    )
    .bind(owner_user_id)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.map(LlmProviderSettings::try_from).transpose()
}

pub(crate) async fn upsert_llm_provider_settings(
    storage: &Storage,
    owner_user_id: &str,
    request: UpdateLlmProviderSettingsRequest,
) -> Result<LlmProviderSettings, StorageError> {
    let record = ValidatedLlmProviderSettings::from_request(request)?;
    let encrypted_api_key = record
        .api_key
        .map(|api_key| storage.cipher.encrypt(&api_key))
        .transpose()?;
    let row = sqlx::query_as::<_, LlmProviderSettingsRow>(
        r#"
        insert into user_llm_provider_settings (
            owner_user_id, provider, base_url, model, api_mode, streaming_enabled,
            encrypted_api_key, updated_at
        )
        values (
            $1::uuid, $2, $3, $4, $5, $6, coalesce($7::text, ''), now()
        )
        on conflict (owner_user_id) do update
        set provider = excluded.provider,
            base_url = excluded.base_url,
            model = excluded.model,
            api_mode = excluded.api_mode,
            streaming_enabled = excluded.streaming_enabled,
            encrypted_api_key = coalesce($7::text, user_llm_provider_settings.encrypted_api_key),
            updated_at = now()
        returning provider, base_url, model, api_mode, streaming_enabled,
            encrypted_api_key <> '' as has_api_key
        "#,
    )
    .bind(owner_user_id)
    .bind(record.provider.as_str())
    .bind(record.base_url)
    .bind(record.model)
    .bind(record.api_mode.as_str())
    .bind(record.streaming_enabled)
    .bind(encrypted_api_key)
    .fetch_one(&storage.pool)
    .await
    .map_err(map_database_error)?;

    LlmProviderSettings::try_from(row)
}

pub(crate) async fn resolve_llm_provider_settings(
    storage: &Storage,
    owner_user_id: &str,
) -> Result<Option<ResolvedLlmProviderSettings>, StorageError> {
    let row = sqlx::query_as::<_, ResolvedLlmProviderSettingsRow>(
        r#"
        select provider, base_url, model, api_mode, streaming_enabled, encrypted_api_key
        from user_llm_provider_settings
        where owner_user_id = $1::uuid
        "#,
    )
    .bind(owner_user_id)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.map(|row| row.into_resolved(storage)).transpose()
}

#[derive(Debug)]
struct ValidatedLlmProviderSettings {
    provider: LlmProviderKind,
    base_url: String,
    model: String,
    api_mode: LlmProviderApiMode,
    streaming_enabled: bool,
    api_key: Option<String>,
}

impl ValidatedLlmProviderSettings {
    fn from_request(request: UpdateLlmProviderSettingsRequest) -> Result<Self, StorageError> {
        Ok(Self {
            provider: request.provider,
            base_url: required_string("base_url", &request.base_url)?,
            model: required_string("model", &request.model)?,
            api_mode: request.api_mode,
            streaming_enabled: request.streaming_enabled.unwrap_or(true),
            api_key: request
                .api_key
                .map(|api_key| api_key.trim().to_owned())
                .filter(|api_key| !api_key.is_empty()),
        })
    }
}

#[derive(Debug)]
struct LlmProviderSettingsRow {
    provider: String,
    base_url: String,
    model: String,
    api_mode: String,
    streaming_enabled: bool,
    has_api_key: bool,
}

#[derive(Debug)]
struct ResolvedLlmProviderSettingsRow {
    provider: String,
    base_url: String,
    model: String,
    api_mode: String,
    streaming_enabled: bool,
    encrypted_api_key: String,
}

impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for LlmProviderSettingsRow {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;

        Ok(Self {
            provider: row.try_get("provider")?,
            base_url: row.try_get("base_url")?,
            model: row.try_get("model")?,
            api_mode: row.try_get("api_mode")?,
            streaming_enabled: row.try_get("streaming_enabled")?,
            has_api_key: row.try_get("has_api_key")?,
        })
    }
}

impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for ResolvedLlmProviderSettingsRow {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;

        Ok(Self {
            provider: row.try_get("provider")?,
            base_url: row.try_get("base_url")?,
            model: row.try_get("model")?,
            api_mode: row.try_get("api_mode")?,
            streaming_enabled: row.try_get("streaming_enabled")?,
            encrypted_api_key: row.try_get("encrypted_api_key")?,
        })
    }
}

impl TryFrom<LlmProviderSettingsRow> for LlmProviderSettings {
    type Error = StorageError;

    fn try_from(row: LlmProviderSettingsRow) -> Result<Self, Self::Error> {
        Ok(Self {
            provider: parse_provider(&row.provider)?,
            base_url: row.base_url,
            model: row.model,
            api_mode: parse_api_mode(&row.api_mode)?,
            streaming_enabled: row.streaming_enabled,
            has_api_key: row.has_api_key,
        })
    }
}

impl ResolvedLlmProviderSettingsRow {
    fn into_resolved(self, storage: &Storage) -> Result<ResolvedLlmProviderSettings, StorageError> {
        let api_key = if self.encrypted_api_key.is_empty() {
            None
        } else {
            Some(storage.cipher.decrypt(&self.encrypted_api_key)?)
        };

        Ok(ResolvedLlmProviderSettings {
            provider: parse_provider(&self.provider)?,
            base_url: self.base_url,
            model: self.model,
            api_mode: parse_api_mode(&self.api_mode)?,
            streaming_enabled: self.streaming_enabled,
            api_key,
        })
    }
}

fn parse_provider(value: &str) -> Result<LlmProviderKind, StorageError> {
    match value {
        "openai_compatible" => Ok(LlmProviderKind::OpenaiCompatible),
        other => Err(StorageError::Validation(format!(
            "unsupported llm provider: {other}"
        ))),
    }
}

fn parse_api_mode(value: &str) -> Result<LlmProviderApiMode, StorageError> {
    match value {
        "chat_completions" => Ok(LlmProviderApiMode::ChatCompletions),
        "responses" => Ok(LlmProviderApiMode::Responses),
        other => Err(StorageError::Validation(format!(
            "unsupported llm api mode: {other}"
        ))),
    }
}
