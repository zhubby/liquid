use std::sync::Arc;

use liquid_agent::SqlAuditAgent;
use liquid_storage::LiquidStore;

#[derive(Clone)]
pub struct ApiState {
    pub(crate) agent: Arc<dyn SqlAuditAgent>,
    pub(crate) store: Arc<dyn LiquidStore>,
}

impl ApiState {
    pub fn new(agent: Arc<dyn SqlAuditAgent>, store: Arc<dyn LiquidStore>) -> Self {
        Self { agent, store }
    }
}
