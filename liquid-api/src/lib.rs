use axum::Router;

mod agent_workbench;
mod audit;
mod auth;
mod cors;
mod error;
mod health;
mod managed_databases;
mod server;
mod sql_audits;
mod state;

pub use server::serve;
pub use state::{
    ApiState, ApprovedSqlExecutionFuture, ApprovedSqlExecutor, ManagedDatabaseConnectionTestFuture,
    ManagedDatabaseConnectionTester,
};

pub fn router(state: ApiState) -> Router {
    Router::new()
        .merge(health::routes())
        .merge(agent_workbench::routes())
        .merge(auth::routes())
        .merge(audit::routes())
        .merge(managed_databases::routes())
        .merge(sql_audits::routes())
        .with_state(state)
}

pub fn router_with_cors(state: ApiState, cors_origin: &str) -> anyhow::Result<Router> {
    Ok(router(state).layer(cors::layer(cors_origin)?))
}
