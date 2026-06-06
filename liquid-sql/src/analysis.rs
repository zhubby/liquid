use pg_query::{NodeEnum, protobuf::RawStmt};

use crate::{
    rules,
    types::{
        PgSqlAnalysis, PgSqlAnalysisRequest, PgSqlFinding, PgSqlParseError, PgSqlRiskSeverity,
        PgSqlRuleOptions, PgSqlStatement, PgSqlStatementKind,
    },
};

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
            metadata: None,
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
                metadata: None,
                parse_error: Some(PgSqlParseError { message }),
            };
        }
    };

    let mut analysis = PgSqlAnalysis {
        statements: Vec::new(),
        findings: Vec::new(),
        metadata: None,
        parse_error: None,
    };

    for (index, raw_stmt) in parsed.protobuf.stmts.iter().enumerate() {
        analyze_statement(index, raw_stmt, &request.options, &mut analysis);
    }

    analysis
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

    analysis.statements.push(PgSqlStatement {
        index,
        kind: statement_kind(node),
        location: non_negative(raw_stmt.stmt_location),
        length: non_negative(raw_stmt.stmt_len),
    });

    rules::inspect_statement(index, node, options, analysis);
    rules::inspect_nested_statements(index, node, options, analysis);
}

fn statement_kind(node: &NodeEnum) -> PgSqlStatementKind {
    match node {
        NodeEnum::SelectStmt(_) => PgSqlStatementKind::Select,
        NodeEnum::InsertStmt(_) => PgSqlStatementKind::Insert,
        NodeEnum::UpdateStmt(_) => PgSqlStatementKind::Update,
        NodeEnum::DeleteStmt(_) => PgSqlStatementKind::Delete,
        NodeEnum::MergeStmt(_) => PgSqlStatementKind::Merge,
        NodeEnum::CreateStmt(_)
        | NodeEnum::CreatedbStmt(_)
        | NodeEnum::CreateSchemaStmt(_)
        | NodeEnum::CreateTableAsStmt(_)
        | NodeEnum::IndexStmt(_)
        | NodeEnum::CreateFunctionStmt(_)
        | NodeEnum::CreatePolicyStmt(_)
        | NodeEnum::CreateRoleStmt(_)
        | NodeEnum::CreateExtensionStmt(_) => PgSqlStatementKind::Create,
        NodeEnum::AlterTableStmt(_)
        | NodeEnum::AlterDatabaseStmt(_)
        | NodeEnum::AlterDomainStmt(_)
        | NodeEnum::AlterRoleStmt(_)
        | NodeEnum::AlterRoleSetStmt(_)
        | NodeEnum::AlterFunctionStmt(_)
        | NodeEnum::AlterPolicyStmt(_) => PgSqlStatementKind::Alter,
        NodeEnum::DropStmt(_) | NodeEnum::DropRoleStmt(_) => PgSqlStatementKind::Drop,
        NodeEnum::TruncateStmt(_) => PgSqlStatementKind::Truncate,
        NodeEnum::GrantStmt(_) | NodeEnum::GrantRoleStmt(_) => PgSqlStatementKind::Security,
        NodeEnum::TransactionStmt(_) => PgSqlStatementKind::Transaction,
        NodeEnum::VariableSetStmt(_)
        | NodeEnum::VariableShowStmt(_)
        | NodeEnum::LockStmt(_)
        | NodeEnum::CopyStmt(_)
        | NodeEnum::DoStmt(_)
        | NodeEnum::RefreshMatViewStmt(_) => PgSqlStatementKind::Control,
        _ => PgSqlStatementKind::Other,
    }
}

fn non_negative(value: i32) -> Option<i32> {
    (value >= 0).then_some(value)
}
