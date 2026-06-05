use std::path::PathBuf;
use std::sync::Arc;

use clap::{Args, Parser, Subcommand};
use liquid_agent::{MockSqlAuditAgent, SqlAuditAgent, ToolCallingSqlAuditAgent};
use liquid_config::{LiquidConfig, LlmApiMode};
use liquid_llm::{LlmProtocol, OpenAiCompatibleClient, OpenAiCompatibleConfig};
use liquid_storage::{Storage, StorageOptions};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(
    name = "liquid",
    version,
    about = "Liquid SQL AI audit and BI dashboard"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Start the Liquid API server.
    Server(ConfigArgs),
    /// Run Liquid application database migrations.
    Migrate(ConfigArgs),
    /// Print the Liquid version.
    Version,
}

#[derive(Debug, Args)]
struct ConfigArgs {
    /// Path to a Liquid TOML config file.
    #[arg(short, long, value_name = "PATH")]
    config: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    init_tracing();

    match cli.command {
        Command::Server(args) => run_server(args).await,
        Command::Migrate(args) => run_migrate(args).await,
        Command::Version => {
            println!("liquid {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
    }
}

async fn run_server(args: ConfigArgs) -> anyhow::Result<()> {
    let config = LiquidConfig::from_file_and_env(args.config.as_deref())?;
    let storage = build_storage(&config).await?;
    let agent = build_agent(&config).await?;

    tracing::info!(addr = %config.api_addr, "starting liquid api");
    liquid_api::serve(config, agent, storage).await
}

async fn run_migrate(args: ConfigArgs) -> anyhow::Result<()> {
    let config = LiquidConfig::from_file_and_env(args.config.as_deref())?;
    let storage = connect_storage(&config).await?;

    tracing::info!("running liquid database migrations");
    storage.migrate().await?;
    tracing::info!("liquid database migrations complete");

    Ok(())
}

async fn build_storage(config: &LiquidConfig) -> anyhow::Result<Arc<Storage>> {
    let storage = connect_storage(config).await?;

    if config.database.auto_migrate {
        storage.migrate().await?;
    }

    Ok(Arc::new(storage))
}

async fn connect_storage(config: &LiquidConfig) -> anyhow::Result<Storage> {
    Ok(Storage::connect_with_options(
        StorageOptions::new(config.database.url.clone())
            .with_max_connections(config.database.max_connections)
            .with_token_ttl_seconds(config.auth.token_ttl_seconds)
            .with_encryption_key(config.security.encryption_key.clone()),
    )
    .await?)
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
fn init_tracing() {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("liquid=info"));

    tracing_subscriber::fmt().with_env_filter(env_filter).init();
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[test]
    fn parses_server_config_path() {
        let cli = Cli::parse_from(["liquid", "server", "--config", "liquid.toml"]);

        let Command::Server(args) = cli.command else {
            panic!("expected server command");
        };

        assert_eq!(args.config, Some(PathBuf::from("liquid.toml")));
    }

    #[test]
    fn parses_migrate_config_path() {
        let cli = Cli::parse_from(["liquid", "migrate", "-c", "liquid.toml"]);

        let Command::Migrate(args) = cli.command else {
            panic!("expected migrate command");
        };

        assert_eq!(args.config, Some(PathBuf::from("liquid.toml")));
    }

    #[test]
    fn parses_version_subcommand() {
        let cli = Cli::parse_from(["liquid", "version"]);

        assert!(matches!(cli.command, Command::Version));
    }
}
