use std::sync::Arc;

use liquid_agent::{MockSqlAuditAgent, SqlAuditAgent, ToolCallingSqlAuditAgent, ToolRegistry};
use liquid_config::{LiquidConfig, LlmApiMode, SqlMetadataMode};
use liquid_llm::{LlmProtocol, OpenAiCompatibleClient, OpenAiCompatibleConfig};
use sqlx::postgres::PgPoolOptions;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let config = LiquidConfig::from_env()?;
    let agent = build_agent(&config).await?;

    tracing::info!(addr = %config.api_addr, "starting liquid api");
    liquid_api::serve(config, agent).await
}

async fn build_agent(config: &LiquidConfig) -> anyhow::Result<Arc<dyn SqlAuditAgent>> {
    let Some(api_key) = config.llm.api_key.clone() else {
        tracing::info!("OPENAI_API_KEY is not set; using mock SQL audit agent");
        return Ok(Arc::new(MockSqlAuditAgent));
    };

    let Some(model) = config.llm.model.clone() else {
        tracing::info!("OPENAI_MODEL is not set; using mock SQL audit agent");
        return Ok(Arc::new(MockSqlAuditAgent));
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

    let (metadata_pool, metadata_required) = metadata_pool(config).await?;
    let tools = ToolRegistry::with_sql_metadata_pool(metadata_pool, metadata_required);

    Ok(Arc::new(
        ToolCallingSqlAuditAgent::new(llm, model, protocol).with_tools(tools),
    ))
}

async fn metadata_pool(config: &LiquidConfig) -> anyhow::Result<(Option<sqlx::PgPool>, bool)> {
    match config.sql_metadata {
        SqlMetadataMode::Off => Ok((None, false)),
        SqlMetadataMode::Auto => {
            let pool = PgPoolOptions::new()
                .max_connections(2)
                .connect_lazy(&config.database_url)?;
            Ok((Some(pool), false))
        }
        SqlMetadataMode::Required => {
            let pool = PgPoolOptions::new()
                .max_connections(2)
                .connect(&config.database_url)
                .await?;
            Ok((Some(pool), true))
        }
    }
}

fn init_tracing() {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("liquid=info"));

    tracing_subscriber::fmt().with_env_filter(env_filter).init();
}
