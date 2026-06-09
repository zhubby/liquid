use std::{
    env, fmt, fs,
    path::{Path, PathBuf},
    process,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use anyhow::Context;
use clap::{Args, Parser, Subcommand};
use liquid_agent::{
    MockSqlAuditAgent, PostgresToolExecutionMode, SqlAuditAgent, ToolCallingSqlAuditAgent,
    tools::sets::{sql_audit_tool_names, workbench_readonly_postgres_tool_names},
    workbench_proposal_tool_names,
};
use liquid_config::{
    LiquidConfig, LlmApiMode, SqlExecutionMode, SqlMetadataMode, default_config_toml,
};
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
    about = "Liquid SQL AI audit and datapanel dashboard"
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

struct LoadedConfig {
    config: LiquidConfig,
    path: PathBuf,
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
    let loaded = load_config(&args)?;
    print_server_startup_overview(&loaded.config, &loaded.path);
    print_startup_step(
        "config",
        format!("loaded from {}", loaded.path.display()),
        Duration::ZERO,
    );

    let started = Instant::now();
    let storage = connect_storage(&loaded.config).await?;
    print_startup_step(
        "database",
        format!(
            "connected to {}",
            redact_database_url_password(&loaded.config.database.url)
        ),
        started.elapsed(),
    );

    if loaded.config.database.auto_migrate {
        let started = Instant::now();
        storage
            .migrate()
            .await
            .context("failed to run Liquid application database migrations")?;
        print_startup_step("migrations", "applied".to_owned(), started.elapsed());
    } else {
        print_startup_step("migrations", "skipped by config".to_owned(), Duration::ZERO);
    }

    let started = Instant::now();
    let (agent, agent_info) = build_agent(&loaded.config).await?;
    print_startup_step("agent", agent_info.summary(), started.elapsed());
    for detail in agent_info.tool_set_summaries() {
        print_startup_detail(detail);
    }

    let server_url = server_url(&loaded.config);
    print_startup_step("http", format!("starting at {server_url}"), Duration::ZERO);
    tracing::info!(addr = %loaded.config.api_addr, "starting liquid api");
    liquid_api::serve(loaded.config, agent, Arc::new(storage))
        .await
        .with_context(|| format!("Liquid API server failed at {server_url}"))
}

async fn run_migrate(args: ConfigArgs) -> anyhow::Result<()> {
    let loaded = load_config(&args)?;
    let storage = connect_storage(&loaded.config).await?;

    tracing::info!("running liquid database migrations");
    storage.migrate().await?;
    tracing::info!("liquid database migrations complete");

    Ok(())
}

fn load_config(args: &ConfigArgs) -> anyhow::Result<LoadedConfig> {
    let config_path = config_path_from_args(args, default_config_path)?;
    let config = LiquidConfig::from_file_and_env(Some(&config_path))?;

    Ok(LoadedConfig {
        config,
        path: config_path,
    })
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

async fn connect_storage(config: &LiquidConfig) -> anyhow::Result<Storage> {
    Storage::connect_with_options(
        StorageOptions::new(config.database.url.clone())
            .with_max_connections(config.database.max_connections)
            .with_token_ttl_seconds(config.auth.token_ttl_seconds)
            .with_encryption_key(config.security.encryption_key.clone()),
    )
    .await
    .with_context(|| {
        format!(
            "failed to connect to Liquid application database at {}",
            redact_database_url_password(&config.database.url)
        )
    })
}

fn redact_database_url_password(database_url: &str) -> String {
    redact_url_password(database_url)
}

fn redact_url_password(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return url.to_owned();
    };
    let credentials_start = scheme_end + 3;
    let after_scheme = &url[credentials_start..];
    let Some(credentials_end) = after_scheme.find('@') else {
        return url.to_owned();
    };
    let credentials = &after_scheme[..credentials_end];
    let Some(password_start) = credentials.rfind(':') else {
        return url.to_owned();
    };

    format!(
        "{}{}{}[redacted]@{}",
        &url[..credentials_start],
        &credentials[..password_start],
        &credentials[password_start..=password_start],
        &after_scheme[credentials_end + 1..]
    )
}

async fn build_agent(
    config: &LiquidConfig,
) -> anyhow::Result<(Arc<dyn SqlAuditAgent>, AgentStartupInfo)> {
    let Some(api_key) = config.llm.api_key.clone() else {
        return Ok((
            Arc::new(MockSqlAuditAgent),
            AgentStartupInfo::Mock {
                reason: "OPENAI_API_KEY unset",
                tool_calling_tool_names: ToolCallingSqlAuditAgent::default_tool_names(),
                request_scoped_tool_sets: request_scoped_tool_sets(config),
            },
        ));
    };

    let Some(model) = config.llm.model.clone() else {
        return Ok((
            Arc::new(MockSqlAuditAgent),
            AgentStartupInfo::Mock {
                reason: "OPENAI_MODEL unset",
                tool_calling_tool_names: ToolCallingSqlAuditAgent::default_tool_names(),
                request_scoped_tool_sets: request_scoped_tool_sets(config),
            },
        ));
    };

    let llm = Arc::new(OpenAiCompatibleClient::new(OpenAiCompatibleConfig::new(
        Some(api_key),
        config.llm.base_url.clone(),
    )));
    let protocol = match config.llm.api_mode {
        LlmApiMode::ChatCompletions => LlmProtocol::ChatCompletions,
        LlmApiMode::Responses => LlmProtocol::Responses,
    };

    let agent = ToolCallingSqlAuditAgent::new(llm, model.clone(), protocol);
    let tool_names = agent.tool_names();

    Ok((
        Arc::new(agent) as Arc<dyn SqlAuditAgent>,
        AgentStartupInfo::OpenAiCompatible {
            model,
            api_mode: config.llm.api_mode,
            base_url: config.llm.base_url.clone(),
            tool_names,
            request_scoped_tool_sets: request_scoped_tool_sets(config),
        },
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AgentToolSetStartupInfo {
    label: &'static str,
    tool_names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AgentStartupInfo {
    Mock {
        reason: &'static str,
        tool_calling_tool_names: Vec<String>,
        request_scoped_tool_sets: Vec<AgentToolSetStartupInfo>,
    },
    OpenAiCompatible {
        model: String,
        api_mode: LlmApiMode,
        base_url: String,
        tool_names: Vec<String>,
        request_scoped_tool_sets: Vec<AgentToolSetStartupInfo>,
    },
}

impl AgentStartupInfo {
    fn summary(&self) -> String {
        match self {
            Self::Mock { reason, .. } => format!("mock SQL audit agent ({reason}) active_tools=[]"),
            Self::OpenAiCompatible {
                model,
                api_mode,
                base_url,
                tool_names,
                ..
            } => format!(
                "OpenAI-compatible model={} api_mode={} base_url={} active_tools={}",
                model,
                llm_api_mode_label(*api_mode),
                redact_url_password(base_url),
                registered_tools_label(tool_names)
            ),
        }
    }

    fn tool_set_summaries(&self) -> Vec<String> {
        let (tool_calling_tool_names, request_scoped_tool_sets) = match self {
            Self::Mock {
                tool_calling_tool_names,
                request_scoped_tool_sets,
                ..
            } => (tool_calling_tool_names, request_scoped_tool_sets),
            Self::OpenAiCompatible {
                tool_names,
                request_scoped_tool_sets,
                ..
            } => (tool_names, request_scoped_tool_sets),
        };

        let mut summaries = vec![format!(
            "tool_calling_default={}",
            registered_tools_label(tool_calling_tool_names)
        )];
        summaries.extend(request_scoped_tool_sets.iter().map(|tool_set| {
            format!(
                "{}={}",
                tool_set.label,
                registered_tools_label(&tool_set.tool_names)
            )
        }));

        summaries
    }
}

fn registered_tools_label(tool_names: &[String]) -> String {
    if tool_names.is_empty() {
        return "[]".to_owned();
    }

    format!("[{}]", tool_names.join(", "))
}

fn request_scoped_tool_sets(config: &LiquidConfig) -> Vec<AgentToolSetStartupInfo> {
    let mut workbench_tool_names = workbench_readonly_postgres_tool_names();
    workbench_tool_names.extend(workbench_proposal_tool_names());
    workbench_tool_names.sort();

    vec![
        AgentToolSetStartupInfo {
            label: "managed_database_audit",
            tool_names: sql_audit_tool_names(postgres_tool_execution_mode(config.sql_execution)),
        },
        AgentToolSetStartupInfo {
            label: "workbench",
            tool_names: workbench_tool_names,
        },
    ]
}

fn postgres_tool_execution_mode(mode: SqlExecutionMode) -> PostgresToolExecutionMode {
    match mode {
        SqlExecutionMode::Off => PostgresToolExecutionMode::Off,
        SqlExecutionMode::Readonly => PostgresToolExecutionMode::Readonly,
        SqlExecutionMode::WriteGated => PostgresToolExecutionMode::WriteGated,
    }
}

fn print_server_startup_overview(config: &LiquidConfig, config_path: &Path) {
    println!("{}", render_server_startup_overview(config, config_path));
}

fn render_server_startup_overview(config: &LiquidConfig, config_path: &Path) -> String {
    let rows = [
        (
            "version",
            format!("{} ({})", env!("CARGO_PKG_VERSION"), build_profile()),
        ),
        ("system", system_summary()),
        ("process", process_summary()),
        ("config", config_path.display().to_string()),
        (
            "api",
            format!("{} cors={}", server_url(config), config.cors_origin),
        ),
        (
            "database",
            format!(
                "{} pool={} auto_migrate={}",
                redact_database_url_password(&config.database.url),
                config.database.max_connections,
                enabled_label(config.database.auto_migrate)
            ),
        ),
        (
            "sql",
            format!(
                "metadata={} execution={} managed_pool={} acquire_timeout={}s",
                sql_metadata_mode_label(config.sql_metadata),
                sql_execution_mode_label(config.sql_execution),
                config.managed_database_pool.max_connections,
                config.managed_database_pool.acquire_timeout_seconds
            ),
        ),
        ("llm", llm_config_summary(config)),
        ("workbench", workbench_config_summary(config)),
        ("backups", backup_config_summary(config)),
    ];

    let mut output = String::from("\nLiquid server\n=============\n");
    for (label, value) in rows {
        output.push_str(&format!("  {label:<12} {value}\n"));
    }
    output.push_str("\nInitialization\n--------------");
    output
}

fn print_startup_step(label: &str, detail: String, duration: Duration) {
    println!(
        "  [ok] {label:<12} {detail} ({})",
        format_duration(duration)
    );
}

fn print_startup_detail(detail: String) {
    println!("       {detail}");
}

fn format_duration(duration: Duration) -> String {
    if duration.as_millis() < 1_000 {
        format!("{}ms", duration.as_millis())
    } else {
        format!("{:.2}s", duration.as_secs_f64())
    }
}

fn build_profile() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}

fn system_summary() -> String {
    let cpus = thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1);

    format!("{} {} cpus={cpus}", env::consts::OS, env::consts::ARCH)
}

fn process_summary() -> String {
    let cwd = env::current_dir()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "unknown".to_owned());

    format!("pid={} cwd={cwd}", process::id())
}

fn server_url(config: &LiquidConfig) -> String {
    format!("http://{}", config.api_addr)
}

fn llm_config_summary(config: &LiquidConfig) -> String {
    match (&config.llm.api_key, &config.llm.model) {
        (Some(_), Some(model)) => format!(
            "openai-compatible model={} api_mode={} base_url={}",
            model,
            llm_api_mode_label(config.llm.api_mode),
            redact_url_password(&config.llm.base_url)
        ),
        (None, _) => format!(
            "mock api_mode={} reason=OPENAI_API_KEY unset",
            llm_api_mode_label(config.llm.api_mode)
        ),
        (_, None) => format!(
            "mock api_mode={} reason=OPENAI_MODEL unset",
            llm_api_mode_label(config.llm.api_mode)
        ),
    }
}

fn workbench_config_summary(config: &LiquidConfig) -> String {
    let max_output_tokens = config
        .workbench
        .max_output_tokens
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unlimited".to_owned());

    format!(
        "max_tool_rounds={} max_output_tokens={max_output_tokens}",
        config.workbench.max_tool_rounds
    )
}

fn backup_config_summary(config: &LiquidConfig) -> String {
    let Some(bucket) = config.database_backup.s3_bucket.as_deref() else {
        return format!(
            "local work_dir={} concurrency={}",
            config.database_backup.work_dir, config.database_backup.worker_concurrency
        );
    };

    format!(
        "s3 bucket={} region={} prefix={} work_dir={} concurrency={}",
        bucket,
        config.database_backup.s3_region,
        config.database_backup.s3_prefix,
        config.database_backup.work_dir,
        config.database_backup.worker_concurrency
    )
}

fn enabled_label(enabled: bool) -> &'static str {
    if enabled { "on" } else { "off" }
}

fn llm_api_mode_label(mode: LlmApiMode) -> &'static str {
    match mode {
        LlmApiMode::ChatCompletions => "chat_completions",
        LlmApiMode::Responses => "responses",
    }
}

fn sql_metadata_mode_label(mode: SqlMetadataMode) -> &'static str {
    match mode {
        SqlMetadataMode::Auto => "auto",
        SqlMetadataMode::Off => "off",
        SqlMetadataMode::Required => "required",
    }
}

fn sql_execution_mode_label(mode: SqlExecutionMode) -> &'static str {
    match mode {
        SqlExecutionMode::Off => "off",
        SqlExecutionMode::Readonly => "readonly",
        SqlExecutionMode::WriteGated => "write_gated",
    }
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
    use liquid_config::{
        AuthConfig, DatabaseBackupConfig, DatabaseConfig, LlmConfig, ManagedDatabasePoolConfig,
        SecurityConfig,
    };

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
    fn redacts_database_url_password() {
        assert_eq!(
            redact_database_url_password("postgres://liquid:secret@localhost:5432/liquid"),
            "postgres://liquid:[redacted]@localhost:5432/liquid"
        );
        assert_eq!(
            redact_url_password("https://user:secret@example.test/v1"),
            "https://user:[redacted]@example.test/v1"
        );
        assert_eq!(
            redact_database_url_password("postgres://liquid@localhost:5432/liquid"),
            "postgres://liquid@localhost:5432/liquid"
        );
    }

    #[test]
    fn renders_server_startup_overview_with_redacted_database_url() {
        let config = test_config();
        let rendered = render_server_startup_overview(&config, Path::new("/tmp/liquid.toml"));

        assert!(rendered.contains("Liquid server"));
        assert!(rendered.contains("Initialization"));
        assert!(rendered.contains("postgres://postgres:[redacted]@localhost:5432/liquid"));
        assert!(rendered.contains("metadata=required execution=write_gated"));
        assert!(rendered.contains("openai-compatible model=gpt-test"));
        assert!(rendered.contains("max_tool_rounds=10 max_output_tokens=unlimited"));
        assert!(!rendered.contains("db-secret"));
        assert!(!rendered.contains("llm-secret"));
    }

    #[test]
    fn summarizes_agent_startup_with_redacted_base_url() {
        let info = AgentStartupInfo::OpenAiCompatible {
            model: "gpt-test".to_owned(),
            api_mode: LlmApiMode::Responses,
            base_url: "https://user:llm-secret@example.test/v1".to_owned(),
            tool_names: vec!["inspect_sql_risk".to_owned(), "pg_list_schemas".to_owned()],
            request_scoped_tool_sets: test_tool_sets(),
        };

        let summary = info.summary();

        assert!(summary.contains("api_mode=responses"));
        assert!(summary.contains("https://user:[redacted]@example.test/v1"));
        assert!(summary.contains("active_tools=[inspect_sql_risk, pg_list_schemas]"));
        assert!(!summary.contains("llm-secret"));
    }

    #[test]
    fn summarizes_mock_agent_with_tool_calling_tool_names() {
        let info = AgentStartupInfo::Mock {
            reason: "OPENAI_MODEL unset",
            tool_calling_tool_names: vec!["inspect_sql_risk".to_owned()],
            request_scoped_tool_sets: test_tool_sets(),
        };

        let summary = info.summary();
        let tool_set_summaries = info.tool_set_summaries();

        assert!(summary.contains("mock SQL audit agent (OPENAI_MODEL unset)"));
        assert!(summary.contains("active_tools=[]"));
        assert!(tool_set_summaries.contains(&"tool_calling_default=[inspect_sql_risk]".to_owned()));
        assert!(
            tool_set_summaries
                .contains(&"managed_database_audit=[inspect_sql_risk, pg_list_schemas]".to_owned())
        );
    }

    #[test]
    fn renders_empty_registered_tools_label() {
        assert_eq!(registered_tools_label(&[]), "[]");
    }

    #[test]
    fn formats_startup_durations() {
        assert_eq!(format_duration(Duration::from_millis(42)), "42ms");
        assert_eq!(format_duration(Duration::from_millis(1_250)), "1.25s");
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

    fn test_tool_sets() -> Vec<AgentToolSetStartupInfo> {
        vec![AgentToolSetStartupInfo {
            label: "managed_database_audit",
            tool_names: vec!["inspect_sql_risk".to_owned(), "pg_list_schemas".to_owned()],
        }]
    }

    fn test_config() -> LiquidConfig {
        LiquidConfig {
            api_addr: "127.0.0.1:3001".parse().unwrap(),
            cors_origin: "http://localhost:3000".to_owned(),
            database: DatabaseConfig {
                url: "postgres://postgres:db-secret@localhost:5432/liquid".to_owned(),
                max_connections: 5,
                auto_migrate: true,
            },
            auth: AuthConfig {
                token_ttl_seconds: 604_800,
            },
            security: SecurityConfig {
                encryption_key: "test-encryption-key".to_owned(),
            },
            sql_metadata: SqlMetadataMode::Required,
            sql_execution: SqlExecutionMode::WriteGated,
            managed_database_pool: ManagedDatabasePoolConfig {
                max_connections: 2,
                idle_ttl_seconds: 600,
                reap_interval_seconds: 60,
                acquire_timeout_seconds: 10,
            },
            database_backup: DatabaseBackupConfig {
                s3_bucket: None,
                s3_prefix: "liquid/database-backups".to_owned(),
                s3_region: "us-east-1".to_owned(),
                s3_endpoint: None,
                s3_path_style: false,
                work_dir: "/Users/test/.liquid/backup".to_owned(),
                worker_concurrency: 1,
            },
            llm: LlmConfig {
                api_key: Some("llm-secret".to_owned()),
                base_url: "https://user:llm-secret@example.test/v1".to_owned(),
                model: Some("gpt-test".to_owned()),
                api_mode: LlmApiMode::ChatCompletions,
            },
            workbench: liquid_config::WorkbenchConfig {
                max_tool_rounds: 10,
                max_output_tokens: None,
            },
        }
    }
}
