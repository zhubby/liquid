use std::sync::Arc;

use liquid_agent::SqlAuditAgent;
use liquid_config::LiquidConfig;
use liquid_storage::LiquidStore;
use tokio::net::TcpListener;

use crate::{ApiState, router_with_cors};

pub async fn serve(
    config: LiquidConfig,
    agent: Arc<dyn SqlAuditAgent>,
    store: Arc<dyn LiquidStore>,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(config.api_addr).await?;
    let app = router_with_cors(ApiState::new(agent, store), &config.cors_origin)?;

    axum::serve(listener, app).await?;
    Ok(())
}
