use axum::Router;

mod agent_workbench;
mod audit;
mod auth;
mod chat;
mod chat_sql;
mod cors;
mod database_backups;
mod database_diagram_generation;
mod database_diagrams;
mod datapanels;
mod error;
mod health;
mod llm_provider;
mod managed_databases;
mod server;
mod settings;
mod sql_audits;
mod state;

pub use chat_sql::{ChatSqlExecutionFuture, ChatSqlExecutionOutcome, ChatSqlExecutor};
pub use database_diagram_generation::{
    DatabaseDiagramGenerationFuture, DatabaseDiagramGenerator, PostgresDatabaseDiagramGenerator,
};
pub use server::serve;
pub use state::{
    ApiState, ApprovedSqlExecutionFuture, ApprovedSqlExecutor, ManagedDatabaseConnectionTestFuture,
    ManagedDatabaseConnectionTester,
};

pub fn router(state: ApiState) -> Router {
    Router::new()
        .merge(health::routes())
        .merge(chat::routes())
        .merge(database_backups::routes())
        .merge(database_diagrams::routes())
        .merge(datapanels::routes())
        .merge(auth::routes())
        .merge(audit::routes())
        .merge(managed_databases::routes())
        .merge(settings::routes())
        .merge(sql_audits::routes())
        .with_state(state)
}

pub fn router_with_cors(state: ApiState, cors_origin: &str) -> anyhow::Result<Router> {
    Ok(router(state).layer(cors::layer(cors_origin)?))
}
