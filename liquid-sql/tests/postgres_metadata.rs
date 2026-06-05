use liquid_sql::{PgSqlAnalysisRequest, PgSqlMetadataOptions, analyze_postgres_sql_with_database};
use sqlx::{PgPool, postgres::PgPoolOptions};

async fn test_pool() -> Option<PgPool> {
    let database_url = std::env::var("LIQUID_TEST_DATABASE_URL").ok()?;
    PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url)
        .await
        .ok()
}

#[tokio::test]
async fn postgres_metadata_provider_collects_catalog_and_explain_facts() {
    let Some(pool) = test_pool().await else {
        return;
    };

    sqlx::query(
        r#"
        create temporary table liquid_sql_metadata_users (
            id bigserial primary key,
            email text not null
        )
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    let analysis = analyze_postgres_sql_with_database(
        PgSqlAnalysisRequest::new("select id from liquid_sql_metadata_users"),
        &pool,
        PgSqlMetadataOptions::default(),
    )
    .await;

    assert!(analysis.parse_ok());
    let metadata = analysis.metadata.expect("metadata report");
    assert!(!metadata.statements.is_empty());
    assert!(
        metadata.statements[0]
            .relations
            .iter()
            .any(|relation| relation.name == "liquid_sql_metadata_users")
    );
    assert!(metadata.statements[0].plan.is_some());
}
