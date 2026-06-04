use anyhow::Result;
use async_trait::async_trait;
use liquid_core::AuditSummary;

#[async_trait]
pub trait SqlAuditAgent: Send + Sync {
    async fn audit_summary(&self) -> Result<AuditSummary>;
}

#[derive(Debug, Default)]
pub struct MockSqlAuditAgent;

#[async_trait]
impl SqlAuditAgent for MockSqlAuditAgent {
    async fn audit_summary(&self) -> Result<AuditSummary> {
        Ok(AuditSummary::sample())
    }
}
