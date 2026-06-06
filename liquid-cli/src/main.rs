use std::fmt;
use std::sync::Arc;
use std::{env, fs, path::PathBuf};

use anyhow::Context;
use clap::{Args, Parser, Subcommand};
use liquid_agent::{MockSqlAuditAgent, SqlAuditAgent, ToolCallingSqlAuditAgent};
use liquid_config::{LiquidConfig, LlmApiMode, default_config_toml};
use liquid_llm::{LlmProtocol, OpenAiCompatibleClient, OpenAiCompatibleConfig};
use liquid_storage::{Storage, StorageOptions};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tracing::{
    Event, Level, Subscriber,
    field::{Field, Visit},
};
use tracing_subscriber::{EnvFilter, Layer, layer::Context as LayerContext, prelude::*};

const DEFAULT_CONFIG_DIR: &str = ".liquid";
const DEFAULT_CONFIG_FILE: &str = "config.toml";
const SQLX_LOGS_ENV: &str = "LIQUID_SQLX_LOGS";

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
    let regular_filter = env_filter_from_env().add_directive(
        "sqlx::query=off"
            .parse()
            .expect("valid sqlx query filter directive"),
    );

    let regular_layer = tracing_subscriber::fmt::layer().with_filter(regular_filter);
    let registry = tracing_subscriber::registry().with(regular_layer);

    if sqlx_query_logging_enabled() {
        let sqlx_filter = EnvFilter::new("sqlx::query=debug");
        let sqlx_layer = PrettySqlxQueryLayer.with_filter(sqlx_filter);
        registry.with(sqlx_layer).init();
        tracing::info!(env = SQLX_LOGS_ENV, "pretty SQLx query logging enabled");
    } else {
        registry.init();
    }
}

fn env_filter_from_env() -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("liquid=info"))
}

fn sqlx_query_logging_enabled() -> bool {
    env::var(SQLX_LOGS_ENV)
        .ok()
        .map(|value| truthy_env_switch(&value))
        .unwrap_or(false)
}

fn truthy_env_switch(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

struct PrettySqlxQueryLayer;

impl<S> Layer<S> for PrettySqlxQueryLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _context: LayerContext<'_, S>) {
        if event.metadata().target() != "sqlx::query" {
            return;
        }

        let mut fields = SqlxQueryFields::default();
        event.record(&mut fields);

        eprintln!("{}", fields.render(*event.metadata().level()));
    }
}

#[derive(Debug, Default)]
struct SqlxQueryFields {
    summary: Option<String>,
    statement: Option<String>,
    rows_affected: Option<String>,
    rows_returned: Option<String>,
    elapsed: Option<String>,
}

impl SqlxQueryFields {
    fn render(&self, level: Level) -> String {
        let summary = self.summary.as_deref().unwrap_or("query");
        let mut line = format!(
            "{} {:>5} sqlx::query: {}",
            timestamp_rfc3339(),
            level,
            summary
        );

        if let Some(rows_returned) = self.rows_returned.as_deref() {
            line.push_str(&format!(" rows_returned={rows_returned}"));
        }

        if let Some(rows_affected) = self.rows_affected.as_deref() {
            line.push_str(&format!(" rows_affected={rows_affected}"));
        }

        if let Some(elapsed) = self.elapsed.as_deref() {
            line.push_str(&format!(" elapsed={elapsed}"));
        }

        let Some(statement) = self.statement.as_deref() else {
            return line;
        };

        let statement = normalize_sql_statement(statement);

        if statement.is_empty() {
            return line;
        }

        line.push_str("\n    sql:\n");
        for sql_line in statement.lines() {
            line.push_str("      ");
            line.push_str(sql_line);
            line.push('\n');
        }
        line.pop();

        line
    }

    fn record_value(&mut self, name: &str, value: String) {
        let value = decode_debug_string(value.trim()).unwrap_or(value);

        match name {
            "summary" => self.summary = Some(value),
            "db.statement" => self.statement = Some(value),
            "rows_affected" => self.rows_affected = Some(value),
            "rows_returned" => self.rows_returned = Some(value),
            "elapsed" => self.elapsed = Some(value),
            _ => {}
        }
    }
}

impl Visit for SqlxQueryFields {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.record_value(field.name(), format!("{value:?}"));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.record_value(field.name(), value.to_owned());
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.record_value(field.name(), value.to_string());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.record_value(field.name(), value.to_string());
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.record_value(field.name(), value.to_string());
    }
}

fn timestamp_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "unknown-time".to_owned())
}

fn decode_debug_string(value: &str) -> Option<String> {
    if !value.starts_with('"') || !value.ends_with('"') {
        return None;
    }

    serde_json::from_str::<String>(value).ok()
}

fn normalize_sql_statement(statement: &str) -> String {
    let mut lines = statement.lines().collect::<Vec<_>>();

    while lines
        .first()
        .map(|line| line.trim().is_empty())
        .unwrap_or(false)
    {
        lines.remove(0);
    }

    while lines
        .last()
        .map(|line| line.trim().is_empty())
        .unwrap_or(false)
    {
        lines.pop();
    }

    let indent = lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            line.chars()
                .take_while(|character| character.is_whitespace())
                .count()
        })
        .min()
        .unwrap_or(0);

    lines
        .into_iter()
        .map(|line| line.chars().skip(indent).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
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
    fn decodes_debug_string_values() {
        assert_eq!(
            decode_debug_string(r#""select \n  1""#).as_deref(),
            Some("select \n  1")
        );
        assert_eq!(decode_debug_string("select 1"), None);
    }

    #[test]
    fn normalizes_sql_statement_indentation() {
        let sql =
            "\n\n        select\n            id,\n            email\n        from users\n    \n";

        assert_eq!(
            normalize_sql_statement(sql),
            "select\n    id,\n    email\nfrom users"
        );
    }

    #[test]
    fn parses_sqlx_log_switch_values() {
        for value in ["1", "true", "TRUE", "yes", "on", " on "] {
            assert!(truthy_env_switch(value));
        }

        for value in ["", "0", "false", "off", "debug"] {
            assert!(!truthy_env_switch(value));
        }
    }

    #[test]
    fn renders_sqlx_query_as_multiline_sql() {
        let fields = SqlxQueryFields {
            summary: Some("select id, email, ...".to_owned()),
            statement: Some(
                "\n\n    select\n        id,\n        email\n    from users\n".to_owned(),
            ),
            rows_affected: Some("0".to_owned()),
            rows_returned: Some("2".to_owned()),
            elapsed: Some("3.4ms".to_owned()),
        };

        let rendered = fields.render(Level::DEBUG);

        assert!(rendered.contains("sqlx::query: select id, email, ..."));
        assert!(rendered.contains("rows_returned=2"));
        assert!(rendered.contains("elapsed=3.4ms"));
        assert!(rendered.contains("\n    sql:\n      select\n          id,"));
        assert!(!rendered.contains(r"\n"));
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
