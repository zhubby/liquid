use std::collections::BTreeMap;

use crate::{
    PgSqlAnalysis, PgSqlAnalysisRequest, PgSqlColumnMetadata, PgSqlConstraintMetadata,
    PgSqlIndexMetadata, PgSqlLockMetadata, PgSqlMetadataError, PgSqlMetadataOptions,
    PgSqlPlanMetadata, PgSqlPlanNodeMetadata, PgSqlPrivilegeMetadata, PgSqlRelationMetadata,
    PgSqlRlsMetadata, PgSqlRuleOptions, PgSqlStatementKind, PgSqlStatementMetadata,
    analyze_postgres_sql, analyze_postgres_sql_with_metadata,
};

use super::metadata::tests::MockMetadataProvider;

fn analyze(sql: &str) -> PgSqlAnalysis {
    analyze_postgres_sql(PgSqlAnalysisRequest::new(sql))
}

fn analyze_with_options(sql: &str, options: PgSqlRuleOptions) -> PgSqlAnalysis {
    analyze_postgres_sql(PgSqlAnalysisRequest::new(sql).with_options(options))
}

fn has_rule(analysis: &PgSqlAnalysis, rule_id: &str) -> bool {
    analysis
        .findings
        .iter()
        .any(|finding| finding.rule_id == rule_id)
}

fn rule_count(analysis: &PgSqlAnalysis, rule_id: &str) -> usize {
    analysis
        .findings
        .iter()
        .filter(|finding| finding.rule_id == rule_id)
        .count()
}

#[test]
fn valid_postgres_sql_is_parsed_and_classified() {
    let analysis = analyze("select id from users; update users set name = 'Ada' where id = 1");

    assert!(analysis.parse_ok());
    assert_eq!(analysis.statements.len(), 2);
    assert_eq!(analysis.statements[0].kind, PgSqlStatementKind::Select);
    assert_eq!(analysis.statements[1].kind, PgSqlStatementKind::Update);
}

#[test]
fn parse_errors_are_reported() {
    let analysis = analyze("select from where");

    assert!(!analysis.parse_ok());
    assert!(analysis.parse_error.is_some());
    assert!(has_rule(&analysis, "parse_error"));
}

#[test]
fn delete_without_where_is_flagged() {
    let analysis = analyze("delete from users");

    assert!(has_rule(&analysis, "delete_without_where"));
    assert_eq!(analysis.risk_floor(), 95);
}

#[test]
fn update_without_where_is_flagged() {
    let analysis = analyze("update users set role = 'admin'");

    assert!(has_rule(&analysis, "update_without_where"));
}

#[test]
fn scoped_dml_is_not_flagged_for_missing_where() {
    let analysis =
        analyze("delete from users where id = 1; update users set role = 'admin' where id = 1");

    assert!(!has_rule(&analysis, "delete_without_where"));
    assert!(!has_rule(&analysis, "update_without_where"));
}

#[test]
fn constant_true_where_is_flagged() {
    let analysis =
        analyze("delete from users where true; update users set role = 'admin' where 1 = 1");

    assert!(has_rule(&analysis, "tautological_where"));
}

#[test]
fn destructive_drop_and_truncate_are_flagged() {
    let drop_table = analyze("drop table users");
    let drop_schema = analyze("drop schema audit");
    let truncate = analyze("truncate table users");

    assert!(has_rule(&drop_table, "destructive_drop"));
    assert!(has_rule(&drop_schema, "destructive_drop"));
    assert!(has_rule(&truncate, "destructive_truncate"));
}

#[test]
fn alter_table_is_flagged() {
    let analysis = analyze("alter table users add column archived_at timestamptz");

    assert!(has_rule(&analysis, "dangerous_alter_table"));
}

#[test]
fn create_table_as_select_is_flagged() {
    let analysis = analyze("create table user_export as select * from users");

    assert!(has_rule(&analysis, "create_table_as_select"));
    assert!(has_rule(&analysis, "select_star"));
}

#[test]
fn select_star_is_flagged() {
    let analysis = analyze("select * from users");

    assert!(has_rule(&analysis, "select_star"));
}

#[test]
fn aggregate_star_and_exists_subquery_are_not_broad_projection() {
    let aggregate = analyze("select count(*) from users");
    let exists = analyze("select exists(select * from users)");

    assert!(!has_rule(&aggregate, "select_star"));
    assert!(!has_rule(&exists, "select_star"));
}

#[test]
fn set_operation_selects_are_inspected() {
    let analysis = analyze("select * from users union all select * from archived_users");

    assert_eq!(rule_count(&analysis, "select_star"), 2);
}

#[test]
fn select_for_update_is_flagged() {
    let analysis = analyze("select id from users for update");

    assert!(has_rule(&analysis, "select_for_locking"));
}

#[test]
fn cross_join_is_flagged_when_ast_has_no_qualification() {
    let analysis = analyze("select users.id from users cross join orders");

    assert!(has_rule(&analysis, "join_without_qualification"));
}

#[test]
fn qualified_join_is_not_flagged() {
    let analysis = analyze("select users.id from users join orders on orders.user_id = users.id");

    assert!(!has_rule(&analysis, "join_without_qualification"));
}

#[test]
fn insert_values_row_limit_is_flagged() {
    let analysis = analyze_with_options(
        "insert into users(id) values (1), (2), (3)",
        PgSqlRuleOptions {
            max_insert_rows: 2,
            ..PgSqlRuleOptions::default()
        },
    );

    assert!(has_rule(&analysis, "insert_values_row_limit"));
}

#[test]
fn insert_select_is_flagged() {
    let analysis = analyze("insert into user_archive select * from users");

    assert!(has_rule(&analysis, "insert_from_select"));
    assert!(has_rule(&analysis, "select_star"));
}

#[test]
fn nested_cte_and_subselect_are_inspected() {
    let cte = analyze("with exported as (select * from users) select id from exported");
    let subselect = analyze("select u.id from (select * from users) u cross join orders");

    assert!(has_rule(&cte, "select_star"));
    assert!(has_rule(&subselect, "select_star"));
    assert!(has_rule(&subselect, "join_without_qualification"));
    assert_eq!(rule_count(&cte, "select_star"), 1);
    assert_eq!(rule_count(&subselect, "select_star"), 1);
}

#[test]
fn modifying_ctes_are_inspected() {
    let analysis = analyze("with purged as (delete from users returning id) select id from purged");

    assert!(has_rule(&analysis, "delete_without_where"));
}

#[test]
fn dml_source_joins_are_inspected() {
    let update = analyze(
        "update users set role = 'admin' from admins cross join teams where admins.id = users.id",
    );

    assert!(has_rule(&update, "join_without_qualification"));
}

#[test]
fn update_delete_sources_are_inspected_for_nested_query_risks() {
    let update = analyze(
        "update users set role = 'admin' from (select * from admins) admins where admins.id = users.id",
    );
    let delete = analyze(
        "delete from users using (select * from banned_users) banned where banned.id = users.id",
    );

    assert!(has_rule(&update, "select_star"));
    assert!(has_rule(&delete, "select_star"));
    assert!(!has_rule(&update, "update_without_where"));
    assert!(!has_rule(&delete, "delete_without_where"));
}

#[test]
fn merge_is_classified_and_write_actions_are_flagged() {
    let analysis = analyze(
        "merge into users using (select * from incoming_users) incoming on users.id = incoming.id when matched then update set email = incoming.email",
    );

    assert_eq!(analysis.statements[0].kind, PgSqlStatementKind::Merge);
    assert!(has_rule(&analysis, "merge_write_actions"));
    assert!(has_rule(&analysis, "select_star"));
}

#[test]
fn copy_from_and_program_are_flagged() {
    let copy_from = analyze("copy users from '/tmp/users.csv' with csv");
    let copy_program = analyze("copy users from program 'cat /tmp/users.csv'");

    assert!(has_rule(&copy_from, "copy_from"));
    assert!(has_rule(&copy_program, "copy_program"));
}

#[test]
fn explicit_lock_is_flagged() {
    let analysis = analyze("lock table users in access exclusive mode");

    assert!(has_rule(&analysis, "explicit_lock"));
}

#[test]
fn postgresql_ddl_operation_risks_are_flagged() {
    let create_index = analyze("create index idx_users_email on users(email)");
    let create_index_concurrently =
        analyze("create index concurrently idx_users_email on users(email)");
    let refresh_matview = analyze("refresh materialized view user_stats");
    let create_extension = analyze("create extension pg_stat_statements");
    let create_function = analyze(
        "create function audit_user() returns trigger language plpgsql as $$ begin return new; end $$",
    );
    let drop_cascade = analyze("drop table users cascade");
    let alter_drop = analyze("alter table users drop column legacy_id");
    let alter_disable_rls = analyze("alter table users disable row level security");

    assert!(has_rule(&create_index, "create_index_without_concurrently"));
    assert!(!has_rule(
        &create_index_concurrently,
        "create_index_without_concurrently"
    ));
    assert!(has_rule(
        &refresh_matview,
        "refresh_matview_without_concurrently"
    ));
    assert!(has_rule(&create_extension, "create_extension"));
    assert!(has_rule(&create_function, "create_function"));
    assert!(has_rule(&drop_cascade, "drop_cascade"));
    assert!(has_rule(&alter_drop, "alter_table_drop_object"));
    assert!(has_rule(&alter_disable_rls, "alter_table_disables_safety"));
}

#[test]
fn security_and_procedural_control_statements_are_flagged() {
    let grant = analyze("grant select on table users to analyst");
    let revoke = analyze("revoke select on table users from analyst");
    let grant_role = analyze("grant admin_role to app_user");
    let alter_role = analyze("alter role app_user createdb");
    let alter_role_set = analyze("alter role app_user set statement_timeout = '5s'");
    let drop_role = analyze("drop role old_app_user");
    let do_block = analyze("do $$ begin perform 1; end $$");

    assert!(has_rule(&grant, "grant_privileges"));
    assert!(has_rule(&revoke, "revoke_privileges"));
    assert!(has_rule(&grant_role, "grant_role"));
    assert!(has_rule(&alter_role, "alter_role"));
    assert!(has_rule(&alter_role_set, "alter_role_set"));
    assert!(has_rule(&drop_role, "drop_role"));
    assert!(has_rule(&do_block, "do_block"));
}

#[tokio::test]
async fn metadata_unavailable_preserves_ast_findings() {
    let provider = MockMetadataProvider {
        error: Some(PgSqlMetadataError::new("database unavailable")),
        ..MockMetadataProvider::default()
    };
    let analysis = analyze_postgres_sql_with_metadata(
        PgSqlAnalysisRequest::new("delete from users"),
        &provider,
        PgSqlMetadataOptions::default(),
    )
    .await;

    assert!(has_rule(&analysis, "delete_without_where"));
    assert!(has_rule(&analysis, "metadata_unavailable"));
    assert_eq!(
        analysis.metadata.unwrap().warnings[0],
        "database unavailable"
    );
}

#[tokio::test]
async fn metadata_flags_large_table_missing_privilege_rls_and_lock_conflict() {
    let mut statement = PgSqlStatementMetadata::new(0);
    statement.relations.push(relation(42, "public", "users"));
    statement.privileges.push(PgSqlPrivilegeMetadata {
        relation_oid: 42,
        action: "DELETE".to_owned(),
        allowed: false,
    });
    statement.rls.push(PgSqlRlsMetadata {
        relation_oid: 42,
        enabled: true,
        forced: false,
        current_role_bypasses_rls: false,
        policy_count: 1,
        applicable_policy_count: 0,
    });
    statement.locks.push(PgSqlLockMetadata {
        relation_oid: 42,
        expected_mode: "RowExclusiveLock".to_owned(),
        conflicting_granted_locks: 1,
        conflicting_waiting_locks: 0,
        longest_conflict_age_ms: Some(12_000),
    });

    let analysis =
        analyze_with_statement_metadata("delete from users where id = 1", statement).await;

    assert!(has_rule(&analysis, "large_table_operation"));
    assert!(has_rule(&analysis, "missing_privilege"));
    assert!(has_rule(&analysis, "rls_without_applicable_policy"));
    assert!(has_rule(&analysis, "lock_conflict"));
}

#[tokio::test]
async fn metadata_flags_plan_cost_and_rows() {
    let mut statement = PgSqlStatementMetadata::new(0);
    statement.plan = Some(PgSqlPlanMetadata {
        statement_index: 0,
        total_cost: 250_000.0,
        plan_rows: 250_000,
        nodes: vec![
            PgSqlPlanNodeMetadata {
                node_type: "Seq Scan".to_owned(),
                relation_name: Some("users".to_owned()),
                total_cost: 200_000.0,
                plan_rows: 250_000,
            },
            PgSqlPlanNodeMetadata {
                node_type: "Nested Loop".to_owned(),
                relation_name: None,
                total_cost: 250_000.0,
                plan_rows: 250_000,
            },
        ],
    });

    let analysis = analyze_with_statement_metadata("select id from users", statement).await;

    assert!(has_rule(&analysis, "high_estimated_rows"));
    assert!(has_rule(&analysis, "high_plan_cost"));
    assert!(has_rule(&analysis, "large_seq_scan"));
    assert!(has_rule(&analysis, "high_cost_nested_loop"));
}

#[tokio::test]
async fn metadata_flags_high_estimated_write_rows_only_for_write_statements() {
    let mut statement = PgSqlStatementMetadata::new(0);
    statement.plan = Some(PgSqlPlanMetadata {
        statement_index: 0,
        total_cost: 10_000.0,
        plan_rows: 250_000,
        nodes: Vec::new(),
    });

    let update = analyze_with_statement_metadata(
        "update users set role = 'member' where active",
        statement.clone(),
    )
    .await;
    let delete =
        analyze_with_statement_metadata("delete from users where inactive", statement.clone())
            .await;
    let insert_select = analyze_with_statement_metadata(
        "insert into archived_users select * from users",
        statement.clone(),
    )
    .await;
    let insert_values = analyze_with_statement_metadata(
        "insert into users(id) values (1), (2), (3)",
        statement.clone(),
    )
    .await;
    let select = analyze_with_statement_metadata("select id from users", statement).await;

    assert!(has_rule(&update, "high_estimated_write_rows"));
    assert!(has_rule(&delete, "high_estimated_write_rows"));
    assert!(has_rule(&insert_select, "high_estimated_write_rows"));
    assert!(!has_rule(&insert_values, "high_estimated_write_rows"));
    assert!(!has_rule(&select, "high_estimated_write_rows"));
}

#[tokio::test]
async fn metadata_flags_duplicate_and_invalid_indexes() {
    let mut statement = PgSqlStatementMetadata::new(0);
    statement.relations.push(relation(42, "public", "users"));
    statement.indexes.push(PgSqlIndexMetadata {
        relation_oid: 42,
        index_oid: 99,
        schema: "public".to_owned(),
        name: "idx_users_email".to_owned(),
        columns: vec!["email".to_owned()],
        is_unique: false,
        is_primary: false,
        is_valid: false,
        is_ready: true,
        predicate: None,
        definition: "CREATE INDEX idx_users_email ON users(email)".to_owned(),
    });

    let analysis = analyze_with_statement_metadata(
        "create index idx_users_email_2 on users(email)",
        statement,
    )
    .await;

    assert!(has_rule(&analysis, "duplicate_index"));
    assert!(has_rule(&analysis, "index_not_ready"));
}

#[tokio::test]
async fn metadata_flags_protective_constraint_drops_and_large_table_validation() {
    let mut statement = PgSqlStatementMetadata::new(0);
    statement.relations.push(relation(42, "public", "users"));
    statement.constraints.push(PgSqlConstraintMetadata {
        relation_oid: 42,
        name: "users_org_id_fkey".to_owned(),
        kind: "f".to_owned(),
        columns: vec!["org_id".to_owned()],
        is_validated: true,
        definition: Some("FOREIGN KEY (org_id) REFERENCES orgs(id)".to_owned()),
    });

    let drop = analyze_with_statement_metadata(
        "alter table users drop constraint users_org_id_fkey",
        statement.clone(),
    )
    .await;
    let validate = analyze_with_statement_metadata(
        "alter table users validate constraint users_org_id_fkey",
        statement.clone(),
    )
    .await;
    let set_not_null = analyze_with_statement_metadata(
        "alter table users alter column org_id set not null",
        statement,
    )
    .await;

    assert!(has_rule(&drop, "drop_protective_constraint"));
    assert!(has_rule(&validate, "large_table_schema_validation"));
    assert!(has_rule(&set_not_null, "large_table_schema_validation"));
}

#[tokio::test]
async fn metadata_flags_foreign_key_without_covering_index_on_large_tables() {
    let mut statement = PgSqlStatementMetadata::new(0);
    statement.relations.push(relation(42, "public", "users"));
    statement.constraints.push(PgSqlConstraintMetadata {
        relation_oid: 42,
        name: "users_org_id_fkey".to_owned(),
        kind: "f".to_owned(),
        columns: vec!["org_id".to_owned(), "team_id".to_owned()],
        is_validated: true,
        definition: Some("FOREIGN KEY (org_id, team_id) REFERENCES teams(org_id, id)".to_owned()),
    });

    let missing =
        analyze_with_statement_metadata("select id from users where org_id = 1", statement.clone())
            .await;

    statement.indexes.push(PgSqlIndexMetadata {
        relation_oid: 42,
        index_oid: 100,
        schema: "public".to_owned(),
        name: "idx_users_org_team".to_owned(),
        columns: vec!["org_id".to_owned(), "team_id".to_owned()],
        is_unique: false,
        is_primary: false,
        is_valid: true,
        is_ready: true,
        predicate: None,
        definition: "CREATE INDEX idx_users_org_team ON users(org_id, team_id)".to_owned(),
    });
    let covered =
        analyze_with_statement_metadata("select id from users where org_id = 1", statement).await;

    assert!(has_rule(&missing, "foreign_key_without_index"));
    assert!(!has_rule(&covered, "foreign_key_without_index"));
}

#[test]
fn insert_on_conflict_update_is_flagged_but_do_nothing_is_not() {
    let update = analyze(
        "insert into users(id, email) values (1, 'a@b.test') on conflict (id) do update set email = excluded.email where users.email is distinct from excluded.email",
    );
    let nothing =
        analyze("insert into users(id, email) values (1, 'a@b.test') on conflict (id) do nothing");

    assert!(has_rule(&update, "insert_on_conflict_update"));
    assert!(!has_rule(&nothing, "insert_on_conflict_update"));
}

#[tokio::test]
async fn metadata_flags_insert_nullable_violations() {
    let mut statement = PgSqlStatementMetadata::new(0);
    statement.relations.push(relation(42, "public", "users"));
    statement.columns.push(PgSqlColumnMetadata {
        relation_oid: 42,
        name: "email".to_owned(),
        is_nullable: false,
        has_default: false,
        is_identity: false,
        is_generated: false,
    });

    let missing =
        analyze_with_statement_metadata("insert into users(id) values (1)", statement.clone())
            .await;
    let null =
        analyze_with_statement_metadata("insert into users(email) values (null)", statement).await;

    assert!(has_rule(&missing, "insert_missing_required_column"));
    assert!(has_rule(&null, "insert_null_into_not_null"));
}

#[tokio::test]
async fn metadata_preserves_explicit_insert_column_order_for_null_checks() {
    let mut statement = PgSqlStatementMetadata::new(0);
    statement.relations.push(relation(42, "public", "users"));
    statement.columns.push(PgSqlColumnMetadata {
        relation_oid: 42,
        name: "email".to_owned(),
        is_nullable: false,
        has_default: false,
        is_identity: false,
        is_generated: false,
    });
    statement.columns.push(PgSqlColumnMetadata {
        relation_oid: 42,
        name: "name".to_owned(),
        is_nullable: true,
        has_default: false,
        is_identity: false,
        is_generated: false,
    });

    let analysis = analyze_with_statement_metadata(
        "insert into users(name, email) values (null, 'a@b.test')",
        statement,
    )
    .await;

    assert!(!has_rule(&analysis, "insert_null_into_not_null"));
}

#[tokio::test]
async fn metadata_flags_missing_predicate_and_join_indexes_on_large_tables() {
    let mut statement = PgSqlStatementMetadata::new(0);
    statement.relations.push(relation(42, "public", "users"));
    statement.relations.push(relation(43, "public", "orders"));
    statement.columns.push(column(42, "id"));
    statement.columns.push(column(42, "email"));
    statement.columns.push(column(43, "user_id"));

    let analysis = analyze_with_statement_metadata(
        "select users.id from users join orders on orders.user_id = users.id where users.email = 'a@b.test'",
        statement,
    )
    .await;

    assert!(has_rule(&analysis, "missing_predicate_index"));
    assert!(has_rule(&analysis, "missing_join_index"));
}

#[tokio::test]
async fn metadata_does_not_flag_missing_index_when_ready_valid_index_exists() {
    let mut statement = PgSqlStatementMetadata::new(0);
    statement.relations.push(relation(42, "public", "users"));
    statement.columns.push(column(42, "email"));
    statement.indexes.push(PgSqlIndexMetadata {
        relation_oid: 42,
        index_oid: 99,
        schema: "public".to_owned(),
        name: "idx_users_email".to_owned(),
        columns: vec!["email".to_owned()],
        is_unique: false,
        is_primary: false,
        is_valid: true,
        is_ready: true,
        predicate: None,
        definition: "CREATE INDEX idx_users_email ON users(email)".to_owned(),
    });

    let analysis =
        analyze_with_statement_metadata("select id from users where email = 'a@b.test'", statement)
            .await;

    assert!(!has_rule(&analysis, "missing_predicate_index"));
}

#[tokio::test]
async fn metadata_uses_table_alias_for_missing_index_detection() {
    let mut statement = PgSqlStatementMetadata::new(0);
    statement.relations.push(relation(42, "public", "users"));
    statement.columns.push(column(42, "email"));

    let analysis = analyze_with_statement_metadata(
        "select u.id from users u where u.email = 'a@b.test'",
        statement,
    )
    .await;

    assert!(has_rule(&analysis, "missing_predicate_index"));
}

#[test]
fn metadata_relation_resolution_excludes_cte_references() {
    let refs = super::postgres::relation_refs_for_test(
        "with users as (select id from archived_users) select id from users",
    );

    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].name, "archived_users");
}

#[tokio::test]
async fn metadata_flags_unvalidated_constraints_and_truncate_rows() {
    let mut statement = PgSqlStatementMetadata::new(0);
    let mut relation = relation(42, "public", "users");
    relation.estimated_rows = Some(12_345.0);
    statement.relations.push(relation);
    statement.constraints.push(PgSqlConstraintMetadata {
        relation_oid: 42,
        name: "users_email_check".to_owned(),
        kind: "c".to_owned(),
        columns: vec!["email".to_owned()],
        is_validated: false,
        definition: Some("CHECK (email <> '') NOT VALID".to_owned()),
    });

    let analysis = analyze_with_statement_metadata("truncate table users", statement).await;

    assert!(has_rule(&analysis, "constraint_not_validated"));
    assert!(has_rule(&analysis, "truncate_estimated_rows"));
}

async fn analyze_with_statement_metadata(
    sql: &str,
    statement: PgSqlStatementMetadata,
) -> PgSqlAnalysis {
    let mut statements = BTreeMap::new();
    statements.insert(0, statement);
    let provider = MockMetadataProvider {
        statements,
        ..MockMetadataProvider::default()
    };

    analyze_postgres_sql_with_metadata(
        PgSqlAnalysisRequest::new(sql),
        &provider,
        PgSqlMetadataOptions::default(),
    )
    .await
}

fn relation(oid: i64, schema: &str, name: &str) -> PgSqlRelationMetadata {
    PgSqlRelationMetadata {
        oid,
        schema: schema.to_owned(),
        name: name.to_owned(),
        kind: "r".to_owned(),
        owner: "postgres".to_owned(),
        total_size_bytes: 2_000_000_000,
        relation_size_bytes: 1_500_000_000,
        estimated_rows: Some(250_000.0),
        live_rows: Some(250_000),
        dead_rows: Some(1_000),
        is_partitioned: false,
        partition_count: 0,
    }
}

fn column(relation_oid: i64, name: &str) -> PgSqlColumnMetadata {
    PgSqlColumnMetadata {
        relation_oid,
        name: name.to_owned(),
        is_nullable: true,
        has_default: false,
        is_identity: false,
        is_generated: false,
    }
}
