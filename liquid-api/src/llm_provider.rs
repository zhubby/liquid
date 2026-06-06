use std::sync::Arc;

use liquid_core::{LlmProviderApiMode, LlmProviderKind};
use liquid_llm::{LlmClient, LlmProtocol, OpenAiCompatibleClient, OpenAiCompatibleConfig};

use crate::{error::ApiError, state::ApiState};

pub(crate) struct UserLlmProvider {
    pub(crate) client: Arc<dyn LlmClient>,
    pub(crate) model: String,
    pub(crate) protocol: LlmProtocol,
}

pub(crate) async fn user_llm_provider_for_user(
    state: &ApiState,
    owner_user_id: &str,
) -> Result<Option<UserLlmProvider>, ApiError> {
    let Some(settings) = state
        .store
        .resolve_llm_provider_settings(owner_user_id)
        .await?
        .filter(|settings| settings.api_key.is_some())
    else {
        return Ok(None);
    };

    let LlmProviderKind::OpenaiCompatible = settings.provider;
    let protocol = match settings.api_mode {
        LlmProviderApiMode::ChatCompletions => LlmProtocol::ChatCompletions,
        LlmProviderApiMode::Responses => LlmProtocol::Responses,
    };
    let client = Arc::new(OpenAiCompatibleClient::new(OpenAiCompatibleConfig::new(
        settings.api_key,
        settings.base_url,
    )));

    Ok(Some(UserLlmProvider {
        client,
        model: settings.model,
        protocol,
    }))
}
