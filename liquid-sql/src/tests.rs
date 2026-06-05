use crate::{
    PgSqlAnalysis, PgSqlAnalysisRequest, PgSqlRuleOptions, PgSqlStatementKind, analyze_postgres_sql,
};

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
