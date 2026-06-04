use std::sync::Arc;

use liquid_agent::{MockSqlAuditAgent, SqlAuditAgent, ToolCallingSqlAuditAgent};
use liquid_config::{LiquidConfig, LlmApiMode};
use liquid_llm::{LlmProtocol, OpenAiCompatibleClient, OpenAiCompatibleConfig};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let config = LiquidConfig::from_env()?;
    let agent = build_agent(&config);

    tracing::info!(addr = %config.api_addr, "starting liquid api");
    liquid_api::serve(config, agent).await
}

fn build_agent(config: &LiquidConfig) -> Arc<dyn SqlAuditAgent> {
    let Some(api_key) = config.llm.api_key.clone() else {
        tracing::info!("OPENAI_API_KEY is not set; using mock SQL audit agent");
        return Arc::new(MockSqlAuditAgent);
    };

    let Some(model) = config.llm.model.clone() else {
        tracing::info!("OPENAI_MODEL is not set; using mock SQL audit agent");
        return Arc::new(MockSqlAuditAgent);
    };

    let llm = Arc::new(OpenAiCompatibleClient::new(OpenAiCompatibleConfig::new(
        Some(api_key),
        config.llm.base_url.clone(),
    )));
    let protocol = match config.llm.api_mode {
        LlmApiMode::ChatCompletions => LlmProtocol::ChatCompletions,
        LlmApiMode::Responses => LlmProtocol::Responses,
    };

    tracing::info!(
        model = %model,
        base_url = %config.llm.base_url,
        api_mode = ?config.llm.api_mode,
        "using OpenAI-compatible SQL audit agent"
    );

    Arc::new(ToolCallingSqlAuditAgent::new(llm, model, protocol))
}

fn init_tracing() {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("liquid=info"));

    tracing_subscriber::fmt().with_env_filter(env_filter).init();
}
