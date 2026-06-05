use std::sync::Arc;
use std::{env, fs, path::PathBuf};

use anyhow::Context;
use clap::{Args, Parser, Subcommand};
use liquid_agent::{MockSqlAuditAgent, SqlAuditAgent, ToolCallingSqlAuditAgent};
use liquid_config::{LiquidConfig, LlmApiMode, default_config_toml};
use liquid_llm::{LlmProtocol, OpenAiCompatibleClient, OpenAiCompatibleConfig};
use liquid_storage::{Storage, StorageOptions};
use tracing_subscriber::EnvFilter;

const DEFAULT_CONFIG_DIR: &str = ".liquid";
const DEFAULT_CONFIG_FILE: &str = "config.toml";

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
    ///
    /// Defaults to ~/.liquid/config.toml and creates that file when it is missing.
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
    let config = load_config(&args)?;
    let storage = build_storage(&config).await?;
    let agent = build_agent(&config).await?;

    tracing::info!(addr = %config.api_addr, "starting liquid api");
    liquid_api::serve(config, agent, storage).await
}

async fn run_migrate(args: ConfigArgs) -> anyhow::Result<()> {
    let config = load_config(&args)?;
    let storage = connect_storage(&config).await?;

    tracing::info!("running liquid database migrations");
    storage.migrate().await?;
    tracing::info!("liquid database migrations complete");

    Ok(())
}

fn load_config(args: &ConfigArgs) -> anyhow::Result<LiquidConfig> {
    let config_path = config_path_from_args(args, default_config_path)?;

    LiquidConfig::from_file_and_env(Some(&config_path))
}

fn config_path_from_args<F>(args: &ConfigArgs, default_path: F) -> anyhow::Result<PathBuf>
where
    F: FnOnce() -> anyhow::Result<PathBuf>,
{
    let Some(config_path) = args.config.as_deref() else {
        return ensure_default_config_file(default_path()?);
    };

    Ok(config_path.to_owned())
}

fn default_config_path() -> anyhow::Result<PathBuf> {
    Ok(home_dir()?
        .join(DEFAULT_CONFIG_DIR)
        .join(DEFAULT_CONFIG_FILE))
}

fn home_dir() -> anyhow::Result<PathBuf> {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .context("could not determine user home directory; pass --config <PATH>")
}

fn ensure_default_config_file(path: PathBuf) -> anyhow::Result<PathBuf> {
    if path.exists() {
        return Ok(path);
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory: {}", parent.display()))?;
    }

    fs::write(&path, default_config_toml())
        .with_context(|| format!("failed to create default config file: {}", path.display()))?;
    tracing::info!(path = %path.display(), "created default liquid config");

    Ok(path)
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
    use std::{
        process,
        time::{SystemTime, UNIX_EPOCH},
    };

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

    #[test]
    fn explicit_config_path_is_used_directly() {
        let args = ConfigArgs {
            config: Some(PathBuf::from("liquid.toml")),
        };
        let path =
            config_path_from_args(&args, || panic!("default path should not be used")).unwrap();

        assert_eq!(path, PathBuf::from("liquid.toml"));
    }

    #[test]
    fn missing_config_creates_default_file() {
        let root = temp_root("liquid-cli-default-config");
        let path = root.join(DEFAULT_CONFIG_DIR).join(DEFAULT_CONFIG_FILE);
        let args = ConfigArgs { config: None };

        let resolved = config_path_from_args(&args, || Ok(path.clone())).unwrap();

        assert_eq!(resolved, path);
        assert!(resolved.exists());
        assert!(resolved.parent().unwrap().is_dir());
        assert!(
            fs::read_to_string(&resolved)
                .unwrap()
                .contains("[database]")
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn existing_default_config_is_preserved() {
        let root = temp_root("liquid-cli-existing-config");
        let path = root.join(DEFAULT_CONFIG_DIR).join(DEFAULT_CONFIG_FILE);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "sentinel = true\n").unwrap();
        let args = ConfigArgs { config: None };

        let resolved = config_path_from_args(&args, || Ok(path.clone())).unwrap();

        assert_eq!(resolved, path);
        assert_eq!(fs::read_to_string(&resolved).unwrap(), "sentinel = true\n");

        let _ = fs::remove_dir_all(root);
    }

    fn temp_root(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        env::temp_dir().join(format!("{name}-{}-{nanos}", process::id()))
    }
}
