use std::sync::Arc;

use liquid_agent::MockSqlAuditAgent;
use liquid_config::LiquidConfig;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let config = LiquidConfig::from_env()?;
    let agent = Arc::new(MockSqlAuditAgent);

    tracing::info!(addr = %config.api_addr, "starting liquid api");
    liquid_api::serve(config, agent).await
}

fn init_tracing() {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("liquid=info"));

    tracing_subscriber::fmt().with_env_filter(env_filter).init();
}
