use std::sync::Arc;
use std::{env, path::PathBuf};

use liquid_agent::{MockSqlAuditAgent, SqlAuditAgent, ToolCallingSqlAuditAgent};
use liquid_config::{LiquidConfig, LlmApiMode};
use liquid_llm::{LlmProtocol, OpenAiCompatibleClient, OpenAiCompatibleConfig};
use liquid_storage::{Storage, StorageOptions};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let config_path = config_path_from_args()?;
    let config = LiquidConfig::from_file_and_env(config_path.as_deref())?;
    let storage = build_storage(&config).await?;
    let agent = build_agent(&config).await?;

    tracing::info!(addr = %config.api_addr, "starting liquid api");
    liquid_api::serve(config, agent, storage).await
}

async fn build_storage(config: &LiquidConfig) -> anyhow::Result<Arc<Storage>> {
    let storage = Arc::new(
        Storage::connect_with_options(
            StorageOptions::new(config.database.url.clone())
                .with_max_connections(config.database.max_connections)
                .with_token_ttl_seconds(config.auth.token_ttl_seconds)
                .with_encryption_key(config.security.encryption_key.clone()),
        )
        .await?,
    );

    if config.database.auto_migrate {
        storage.migrate().await?;
    }

    Ok(storage)
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

    Ok(Arc::new(ToolCallingSqlAuditAgent::new(
        llm, model, protocol,
    )))
}

fn config_path_from_args() -> anyhow::Result<Option<PathBuf>> {
    let mut args = env::args().skip(1);
    let mut config_path = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" | "-c" => {
                let Some(path) = args.next() else {
                    anyhow::bail!("{arg} requires a path");
                };
                config_path = Some(PathBuf::from(path));
            }
            "--help" | "-h" => {
                println!("Usage: liquid [--config <path>]");
                std::process::exit(0);
            }
            other => anyhow::bail!("unknown argument: {other}"),
        }
    }

    Ok(config_path)
}

fn init_tracing() {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("liquid=info"));

    tracing_subscriber::fmt().with_env_filter(env_filter).init();
}
