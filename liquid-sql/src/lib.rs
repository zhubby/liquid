use pg_query::{
    NodeEnum,
    protobuf::{
        AConst, AExpr, AlterTableStmt, BoolExpr, DeleteStmt, DropStmt, InsertStmt, JoinExpr, Node,
        RawStmt, SelectStmt, TruncateStmt, UpdateStmt, a_const,
    },
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PgSqlAnalysisRequest {
    pub sql: String,
    pub options: PgSqlRuleOptions,
}

impl PgSqlAnalysisRequest {
    pub fn new(sql: impl Into<String>) -> Self {
        Self {
            sql: sql.into(),
            options: PgSqlRuleOptions::default(),
        }
    }

    pub fn with_options(mut self, options: PgSqlRuleOptions) -> Self {
        self.options = options;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PgSqlRuleOptions {
    pub max_insert_rows: usize,
    pub check_destructive_ddl: bool,
    pub check_dml_scope: bool,
    pub check_broad_projection: bool,
    pub check_joins: bool,
    pub check_transaction_controls: bool,
}

impl Default for PgSqlRuleOptions {
    fn default() -> Self {
        Self {
            max_insert_rows: 1_000,
            check_destructive_ddl: true,
            check_dml_scope: true,
            check_broad_projection: true,
            check_joins: true,
            check_transaction_controls: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PgSqlAnalysis {
    pub statements: Vec<PgSqlStatement>,
    pub findings: Vec<PgSqlFinding>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parse_error: Option<PgSqlParseError>,
}

impl PgSqlAnalysis {
    pub fn parse_ok(&self) -> bool {
        self.parse_error.is_none()
    }

    pub fn risk_floor(&self) -> u8 {
        self.findings
            .iter()
            .map(|finding| finding.severity.risk_floor())
            .max()
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PgSqlStatement {
    pub index: usize,
    pub kind: PgSqlStatementKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub length: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PgSqlStatementKind {
    Select,
    Insert,
    Update,
    Delete,
    Create,
    Alter,
    Drop,
    Truncate,
    Transaction,
    Control,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PgSqlFinding {
    pub rule_id: String,
    pub severity: PgSqlRiskSeverity,
    pub title: String,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub statement_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PgSqlRiskSeverity {
    Low,
    Medium,
    High,
    Critical,
}

impl PgSqlRiskSeverity {
    pub fn risk_floor(&self) -> u8 {
        match self {
            Self::Low => 25,
            Self::Medium => 50,
            Self::High => 80,
            Self::Critical => 95,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PgSqlParseError {
    pub message: String,
}

pub fn analyze_postgres_sql(request: PgSqlAnalysisRequest) -> PgSqlAnalysis {
    let sql = request.sql.trim();

    if sql.is_empty() {
        return PgSqlAnalysis {
            statements: Vec::new(),
            findings: vec![PgSqlFinding::new(
                "parse_error",
                PgSqlRiskSeverity::High,
                "SQL could not be parsed",
                "The request did not include a non-empty PostgreSQL statement.",
                None,
                None,
            )],
            parse_error: Some(PgSqlParseError {
                message: "SQL audit request must include SQL".to_owned(),
            }),
        };
    }

    let parsed = match pg_query::parse(sql) {
        Ok(parsed) => parsed,
        Err(error) => {
            let message = error.to_string();
            return PgSqlAnalysis {
                statements: Vec::new(),
                findings: vec![PgSqlFinding::new(
                    "parse_error",
                    PgSqlRiskSeverity::High,
                    "SQL could not be parsed",
                    "The SQL text is not valid PostgreSQL syntax.",
                    None,
                    Some(message.clone()),
                )],
                parse_error: Some(PgSqlParseError { message }),
            };
        }
    };

    let mut analysis = PgSqlAnalysis {
        statements: Vec::new(),
        findings: Vec::new(),
        parse_error: None,
    };

    for (index, raw_stmt) in parsed.protobuf.stmts.iter().enumerate() {
        analyze_statement(index, raw_stmt, &request.options, &mut analysis);
    }

    analysis
}

impl PgSqlFinding {
    fn new(
        rule_id: impl Into<String>,
        severity: PgSqlRiskSeverity,
        title: impl Into<String>,
        detail: impl Into<String>,
        statement_index: Option<usize>,
        evidence: Option<String>,
    ) -> Self {
        Self {
            rule_id: rule_id.into(),
            severity,
            title: title.into(),
            detail: detail.into(),
            statement_index,
            evidence,
        }
    }
}

fn analyze_statement(
    index: usize,
    raw_stmt: &RawStmt,
    options: &PgSqlRuleOptions,
    analysis: &mut PgSqlAnalysis,
) {
    let Some(node) = raw_stmt.stmt.as_deref().and_then(|stmt| stmt.node.as_ref()) else {
        analysis.findings.push(PgSqlFinding::new(
            "analysis_warning",
            PgSqlRiskSeverity::Low,
            "Statement could not be inspected",
            "The PostgreSQL parser returned a raw statement without an inspectable AST node.",
            Some(index),
            None,
        ));
        return;
    };

    let kind = statement_kind(node);
    analysis.statements.push(PgSqlStatement {
        index,
        kind,
        location: non_negative(raw_stmt.stmt_location),
        length: non_negative(raw_stmt.stmt_len),
    });

    match node {
        NodeEnum::SelectStmt(stmt) => analyze_select(index, stmt, options, analysis),
        NodeEnum::InsertStmt(stmt) => analyze_insert(index, stmt, options, analysis),
        NodeEnum::UpdateStmt(stmt) => analyze_update(index, stmt, options, analysis),
        NodeEnum::DeleteStmt(stmt) => analyze_delete(index, stmt, options, analysis),
        NodeEnum::DropStmt(stmt) => analyze_drop(index, stmt, options, analysis),
        NodeEnum::TruncateStmt(stmt) => analyze_truncate(index, stmt, options, analysis),
        NodeEnum::AlterTableStmt(stmt) => analyze_alter_table(index, stmt, options, analysis),
        NodeEnum::TransactionStmt(_) if options.check_transaction_controls => {
            analysis.findings.push(PgSqlFinding::new(
                "transaction_control",
                PgSqlRiskSeverity::Low,
                "Transaction control statement",
                "The SQL changes transaction state; review ordering and rollback expectations.",
                Some(index),
                Some("transaction statement".to_owned()),
            ));
        }
        _ => {}
    }
}

fn statement_kind(node: &NodeEnum) -> PgSqlStatementKind {
    match node {
        NodeEnum::SelectStmt(_) => PgSqlStatementKind::Select,
        NodeEnum::InsertStmt(_) => PgSqlStatementKind::Insert,
        NodeEnum::UpdateStmt(_) => PgSqlStatementKind::Update,
        NodeEnum::DeleteStmt(_) => PgSqlStatementKind::Delete,
        NodeEnum::CreateStmt(_)
        | NodeEnum::CreateSchemaStmt(_)
        | NodeEnum::CreateTableAsStmt(_)
        | NodeEnum::IndexStmt(_)
        | NodeEnum::CreateFunctionStmt(_)
        | NodeEnum::CreateRoleStmt(_)
        | NodeEnum::CreateExtensionStmt(_) => PgSqlStatementKind::Create,
        NodeEnum::AlterTableStmt(_)
        | NodeEnum::AlterDatabaseStmt(_)
        | NodeEnum::AlterDomainStmt(_)
        | NodeEnum::AlterRoleStmt(_)
        | NodeEnum::AlterFunctionStmt(_) => PgSqlStatementKind::Alter,
        NodeEnum::DropStmt(_) => PgSqlStatementKind::Drop,
        NodeEnum::TruncateStmt(_) => PgSqlStatementKind::Truncate,
        NodeEnum::TransactionStmt(_) => PgSqlStatementKind::Transaction,
        NodeEnum::VariableSetStmt(_)
        | NodeEnum::VariableShowStmt(_)
        | NodeEnum::LockStmt(_)
        | NodeEnum::CopyStmt(_) => PgSqlStatementKind::Control,
        _ => PgSqlStatementKind::Other,
    }
}

fn analyze_select(
    index: usize,
    stmt: &SelectStmt,
    options: &PgSqlRuleOptions,
    analysis: &mut PgSqlAnalysis,
) {
    if options.check_broad_projection && select_has_star(stmt) {
        analysis.findings.push(PgSqlFinding::new(
            "select_star",
            PgSqlRiskSeverity::Medium,
            "Broad column projection",
            "SELECT * returns every visible column and can increase data exposure and scan cost.",
            Some(index),
            Some("SELECT *".to_owned()),
        ));
    }

    if options.check_joins {
        inspect_joins_in_nodes(index, &stmt.from_clause, analysis);
    }

    if let Some(left) = stmt.larg.as_deref() {
        analyze_select(index, left, options, analysis);
    }
    if let Some(right) = stmt.rarg.as_deref() {
        analyze_select(index, right, options, analysis);
    }
}

fn analyze_insert(
    index: usize,
    stmt: &InsertStmt,
    options: &PgSqlRuleOptions,
    analysis: &mut PgSqlAnalysis,
) {
    if options.max_insert_rows == 0 {
        return;
    }

    let Some(select_node) = stmt
        .select_stmt
        .as_deref()
        .and_then(|node| node.node.as_ref())
    else {
        return;
    };
    let NodeEnum::SelectStmt(select) = select_node else {
        return;
    };

    let row_count = select.values_lists.len();
    if row_count > options.max_insert_rows {
        analysis.findings.push(PgSqlFinding::new(
            "insert_values_row_limit",
            PgSqlRiskSeverity::Medium,
            "Large INSERT VALUES batch",
            format!(
                "The INSERT statement contains {row_count} VALUES rows, exceeding the configured limit of {}.",
                options.max_insert_rows
            ),
            Some(index),
            Some(format!("{row_count} rows")),
        ));
    }
}

fn analyze_update(
    index: usize,
    stmt: &UpdateStmt,
    options: &PgSqlRuleOptions,
    analysis: &mut PgSqlAnalysis,
) {
    if !options.check_dml_scope {
        return;
    }

    inspect_dml_where(
        index,
        "update_without_where",
        "UPDATE without WHERE",
        "The UPDATE statement has no WHERE clause and can modify every row in the target relation.",
        stmt.where_clause.as_deref(),
        analysis,
    );
}

fn analyze_delete(
    index: usize,
    stmt: &DeleteStmt,
    options: &PgSqlRuleOptions,
    analysis: &mut PgSqlAnalysis,
) {
    if !options.check_dml_scope {
        return;
    }

    inspect_dml_where(
        index,
        "delete_without_where",
        "DELETE without WHERE",
        "The DELETE statement has no WHERE clause and can remove every row in the target relation.",
        stmt.where_clause.as_deref(),
        analysis,
    );
}

fn inspect_dml_where(
    index: usize,
    missing_rule: &'static str,
    missing_title: &'static str,
    missing_detail: &'static str,
    where_clause: Option<&Node>,
    analysis: &mut PgSqlAnalysis,
) {
    match where_clause {
        None => analysis.findings.push(PgSqlFinding::new(
            missing_rule,
            PgSqlRiskSeverity::Critical,
            missing_title,
            missing_detail,
            Some(index),
            None,
        )),
        Some(node) if is_tautology(node) => analysis.findings.push(PgSqlFinding::new(
            "tautological_where",
            PgSqlRiskSeverity::High,
            "Tautological WHERE clause",
            "The WHERE clause is a constant true expression and does not meaningfully scope the write.",
            Some(index),
            Some("WHERE true / 1 = 1".to_owned()),
        )),
        Some(_) => {}
    }
}

fn analyze_drop(
    index: usize,
    stmt: &DropStmt,
    options: &PgSqlRuleOptions,
    analysis: &mut PgSqlAnalysis,
) {
    if !options.check_destructive_ddl {
        return;
    }

    analysis.findings.push(PgSqlFinding::new(
        "destructive_drop",
        PgSqlRiskSeverity::Critical,
        "Destructive DROP statement",
        "DROP removes database objects and should require explicit review and rollback planning.",
        Some(index),
        Some(format!("remove_type={}", stmt.remove_type)),
    ));
}

fn analyze_truncate(
    index: usize,
    stmt: &TruncateStmt,
    options: &PgSqlRuleOptions,
    analysis: &mut PgSqlAnalysis,
) {
    if !options.check_destructive_ddl {
        return;
    }

    analysis.findings.push(PgSqlFinding::new(
        "destructive_truncate",
        PgSqlRiskSeverity::Critical,
        "Destructive TRUNCATE statement",
        "TRUNCATE removes table contents without row-by-row delete semantics.",
        Some(index),
        Some(format!("relations={}", stmt.relations.len())),
    ));
}

fn analyze_alter_table(
    index: usize,
    stmt: &AlterTableStmt,
    options: &PgSqlRuleOptions,
    analysis: &mut PgSqlAnalysis,
) {
    if !options.check_destructive_ddl {
        return;
    }

    analysis.findings.push(PgSqlFinding::new(
        "dangerous_alter_table",
        PgSqlRiskSeverity::High,
        "Potentially disruptive ALTER TABLE",
        "ALTER TABLE can rewrite data, acquire strong locks, or change application-visible schema.",
        Some(index),
        Some(format!("commands={}", stmt.cmds.len())),
    ));
}

fn select_has_star(stmt: &SelectStmt) -> bool {
    stmt.target_list.iter().any(node_contains_star)
        || stmt
            .larg
            .as_deref()
            .is_some_and(|select| select_has_star(select))
        || stmt
            .rarg
            .as_deref()
            .is_some_and(|select| select_has_star(select))
}

fn node_contains_star(node: &Node) -> bool {
    match node.node.as_ref() {
        Some(NodeEnum::ResTarget(target)) => target
            .val
            .as_deref()
            .is_some_and(|node| node_contains_star(node)),
        Some(NodeEnum::ColumnRef(column)) => column
            .fields
            .iter()
            .any(|field| matches!(field.node.as_ref(), Some(NodeEnum::AStar(_)))),
        Some(NodeEnum::AStar(_)) => true,
        Some(other) => node_children(other)
            .iter()
            .any(|node| node_contains_star(node)),
        None => false,
    }
}

fn inspect_joins_in_nodes(index: usize, nodes: &[Node], analysis: &mut PgSqlAnalysis) {
    for node in nodes {
        inspect_joins_in_node(index, node, analysis);
    }
}

fn inspect_joins_in_node(index: usize, node: &Node, analysis: &mut PgSqlAnalysis) {
    let Some(node_enum) = node.node.as_ref() else {
        return;
    };

    if let NodeEnum::JoinExpr(join) = node_enum {
        inspect_join(index, join, analysis);
    }

    for child in node_children(node_enum) {
        inspect_joins_in_node(index, child, analysis);
    }
}

fn inspect_join(index: usize, join: &JoinExpr, analysis: &mut PgSqlAnalysis) {
    if join.quals.is_none() && join.using_clause.is_empty() && !join.is_natural {
        analysis.findings.push(PgSqlFinding::new(
            "join_without_qualification",
            PgSqlRiskSeverity::Medium,
            "Join without qualification",
            "The join has no ON, USING, or NATURAL qualification in the PostgreSQL AST.",
            Some(index),
            Some(format!("join_type={}", join.jointype)),
        ));
    }
}

fn is_tautology(node: &Node) -> bool {
    match node.node.as_ref() {
        Some(NodeEnum::AConst(constant)) => const_is_true(constant),
        Some(NodeEnum::AExpr(expr)) => a_expr_is_tautology(expr),
        Some(NodeEnum::BoolExpr(expr)) => bool_expr_is_tautology(expr),
        _ => false,
    }
}

fn const_is_true(constant: &AConst) -> bool {
    matches!(
        constant.val.as_ref(),
        Some(a_const::Val::Boolval(value)) if value.boolval
    )
}

fn a_expr_is_tautology(expr: &AExpr) -> bool {
    if operator_name(expr) != Some("=") {
        return false;
    }

    let Some(left) = expr.lexpr.as_deref() else {
        return false;
    };
    let Some(right) = expr.rexpr.as_deref() else {
        return false;
    };

    same_literal(left, right)
}

fn bool_expr_is_tautology(expr: &BoolExpr) -> bool {
    let Ok(bool_op) = pg_query::protobuf::BoolExprType::try_from(expr.boolop) else {
        return false;
    };

    match bool_op {
        pg_query::protobuf::BoolExprType::AndExpr => expr.args.iter().all(is_tautology),
        pg_query::protobuf::BoolExprType::OrExpr => expr.args.iter().any(is_tautology),
        pg_query::protobuf::BoolExprType::NotExpr | pg_query::protobuf::BoolExprType::Undefined => {
            false
        }
    }
}

fn operator_name(expr: &AExpr) -> Option<&str> {
    expr.name.iter().find_map(|node| match node.node.as_ref() {
        Some(NodeEnum::String(value)) => Some(value.sval.as_str()),
        _ => None,
    })
}

fn same_literal(left: &Node, right: &Node) -> bool {
    match (left.node.as_ref(), right.node.as_ref()) {
        (Some(NodeEnum::AConst(left)), Some(NodeEnum::AConst(right))) => {
            literal_key(left) == literal_key(right)
        }
        _ => false,
    }
}

fn literal_key(constant: &AConst) -> Option<String> {
    match constant.val.as_ref()? {
        a_const::Val::Ival(value) => Some(format!("i:{}", value.ival)),
        a_const::Val::Fval(value) => Some(format!("f:{}", value.fval)),
        a_const::Val::Boolval(value) => Some(format!("b:{}", value.boolval)),
        a_const::Val::Sval(value) => Some(format!("s:{}", value.sval)),
        a_const::Val::Bsval(value) => Some(format!("bs:{}", value.bsval)),
    }
}

fn node_children(node: &NodeEnum) -> Vec<&Node> {
    match node {
        NodeEnum::ResTarget(target) => option_node(target.val.as_deref()),
        NodeEnum::SelectStmt(stmt) => {
            let mut children: Vec<&Node> = Vec::new();
            children.extend(stmt.target_list.iter());
            children.extend(stmt.from_clause.iter());
            children.extend(option_node(stmt.where_clause.as_deref()));
            children.extend(stmt.values_lists.iter());
            children.extend(option_select(stmt.larg.as_deref()));
            children.extend(option_select(stmt.rarg.as_deref()));
            children
        }
        NodeEnum::InsertStmt(stmt) => option_node(stmt.select_stmt.as_deref()),
        NodeEnum::UpdateStmt(stmt) => {
            let mut children: Vec<&Node> = Vec::new();
            children.extend(stmt.target_list.iter());
            children.extend(stmt.from_clause.iter());
            children.extend(option_node(stmt.where_clause.as_deref()));
            children
        }
        NodeEnum::DeleteStmt(stmt) => option_node(stmt.where_clause.as_deref()),
        NodeEnum::JoinExpr(join) => {
            let mut children: Vec<&Node> = Vec::new();
            children.extend(option_node(join.larg.as_deref()));
            children.extend(option_node(join.rarg.as_deref()));
            children.extend(join.using_clause.iter());
            children.extend(option_node(join.quals.as_deref()));
            children
        }
        NodeEnum::AExpr(expr) => {
            let mut children: Vec<&Node> = Vec::new();
            children.extend(option_node(expr.lexpr.as_deref()));
            children.extend(option_node(expr.rexpr.as_deref()));
            children
        }
        NodeEnum::BoolExpr(expr) => expr.args.iter().collect(),
        NodeEnum::List(list) => list.items.iter().collect(),
        _ => Vec::new(),
    }
}

fn option_node(node: Option<&Node>) -> Vec<&Node> {
    node.into_iter().collect()
}

fn option_select(select: Option<&SelectStmt>) -> Vec<&Node> {
    match select {
        Some(select) => select
            .target_list
            .iter()
            .chain(select.from_clause.iter())
            .collect(),
        None => Vec::new(),
    }
}

fn non_negative(value: i32) -> Option<i32> {
    (value >= 0).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn select_star_is_flagged() {
        let analysis = analyze("select * from users");

        assert!(has_rule(&analysis, "select_star"));
    }

    #[test]
    fn cross_join_is_flagged_when_ast_has_no_qualification() {
        let analysis = analyze("select users.id from users cross join orders");

        assert!(has_rule(&analysis, "join_without_qualification"));
    }

    #[test]
    fn qualified_join_is_not_flagged() {
        let analysis =
            analyze("select users.id from users join orders on orders.user_id = users.id");

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
}
