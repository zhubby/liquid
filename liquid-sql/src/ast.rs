use pg_query::{
    NodeEnum,
    protobuf::{
        AConst, AExpr, BoolExpr, DeleteStmt, InsertStmt, MergeStmt, Node, SelectStmt, SubLinkType,
        UpdateStmt, WithClause, a_const,
    },
};

pub(crate) fn select_has_star(stmt: &SelectStmt) -> bool {
    stmt.target_list.iter().any(res_target_is_star_projection)
}

fn res_target_is_star_projection(node: &Node) -> bool {
    match node.node.as_ref() {
        Some(NodeEnum::ResTarget(target)) => {
            target.val.as_deref().is_some_and(value_is_star_projection)
        }
        _ => false,
    }
}

fn value_is_star_projection(node: &Node) -> bool {
    match node.node.as_ref() {
        Some(NodeEnum::ColumnRef(column)) => column
            .fields
            .iter()
            .any(|field| matches!(field.node.as_ref(), Some(NodeEnum::AStar(_)))),
        Some(NodeEnum::AStar(_)) => true,
        _ => false,
    }
}

pub(crate) fn is_tautology(node: &Node) -> bool {
    match node.node.as_ref() {
        Some(NodeEnum::AConst(constant)) => const_is_true(constant),
        Some(NodeEnum::AExpr(expr)) => a_expr_is_tautology(expr),
        Some(NodeEnum::BoolExpr(expr)) => bool_expr_is_tautology(expr),
        _ => false,
    }
}

pub(crate) fn node_children(node: &NodeEnum) -> Vec<&Node> {
    match node {
        NodeEnum::ResTarget(target) => option_node(target.val.as_deref()),
        NodeEnum::SelectStmt(stmt) => select_child_nodes(stmt),
        NodeEnum::InsertStmt(stmt) => insert_child_nodes(stmt),
        NodeEnum::UpdateStmt(stmt) => update_child_nodes(stmt),
        NodeEnum::DeleteStmt(stmt) => delete_child_nodes(stmt),
        NodeEnum::MergeStmt(stmt) => merge_child_nodes(stmt),
        NodeEnum::JoinExpr(join) => {
            let mut children: Vec<&Node> = Vec::new();
            children.extend(option_node(join.larg.as_deref()));
            children.extend(option_node(join.rarg.as_deref()));
            children.extend(join.using_clause.iter());
            children.extend(option_node(join.quals.as_deref()));
            children
        }
        NodeEnum::RangeSubselect(subselect) => option_node(subselect.subquery.as_deref()),
        NodeEnum::AExpr(expr) => {
            let mut children: Vec<&Node> = Vec::new();
            children.extend(option_node(expr.lexpr.as_deref()));
            children.extend(option_node(expr.rexpr.as_deref()));
            children
        }
        NodeEnum::SubLink(link) => {
            let mut children: Vec<&Node> = Vec::new();
            children.extend(option_node(link.testexpr.as_deref()));
            if !matches!(
                SubLinkType::try_from(link.sub_link_type),
                Ok(SubLinkType::ExistsSublink)
            ) {
                children.extend(option_node(link.subselect.as_deref()));
            }
            children
        }
        NodeEnum::BoolExpr(expr) => expr.args.iter().collect(),
        NodeEnum::FuncCall(call) => {
            let mut children: Vec<&Node> = Vec::new();
            children.extend(call.args.iter());
            children.extend(call.agg_order.iter());
            children.extend(option_node(call.agg_filter.as_deref()));
            children
        }
        NodeEnum::CaseExpr(expr) => {
            let mut children: Vec<&Node> = Vec::new();
            children.extend(option_node(expr.arg.as_deref()));
            children.extend(expr.args.iter());
            children.extend(option_node(expr.defresult.as_deref()));
            children
        }
        NodeEnum::CaseWhen(expr) => {
            let mut children: Vec<&Node> = Vec::new();
            children.extend(option_node(expr.expr.as_deref()));
            children.extend(option_node(expr.result.as_deref()));
            children
        }
        NodeEnum::AArrayExpr(expr) => expr.elements.iter().collect(),
        NodeEnum::List(list) => list.items.iter().collect(),
        NodeEnum::CreateTableAsStmt(stmt) => option_node(stmt.query.as_deref()),
        NodeEnum::CopyStmt(stmt) => option_node(stmt.query.as_deref()),
        NodeEnum::CommonTableExpr(expr) => option_node(expr.ctequery.as_deref()),
        NodeEnum::MergeWhenClause(clause) => {
            let mut children: Vec<&Node> = Vec::new();
            children.extend(option_node(clause.condition.as_deref()));
            children.extend(clause.target_list.iter());
            children.extend(clause.values.iter());
            children
        }
        _ => Vec::new(),
    }
}

pub(crate) fn select_child_nodes(stmt: &SelectStmt) -> Vec<&Node> {
    let mut children: Vec<&Node> = Vec::new();
    children.extend(stmt.target_list.iter());
    children.extend(stmt.from_clause.iter());
    children.extend(option_node(stmt.where_clause.as_deref()));
    children.extend(stmt.group_clause.iter());
    children.extend(option_node(stmt.having_clause.as_deref()));
    children.extend(stmt.window_clause.iter());
    children.extend(stmt.values_lists.iter());
    children.extend(stmt.sort_clause.iter());
    children.extend(option_node(stmt.limit_offset.as_deref()));
    children.extend(option_node(stmt.limit_count.as_deref()));
    children.extend(stmt.locking_clause.iter());
    children.extend(with_clause_children(stmt.with_clause.as_ref()));
    children
}

pub(crate) fn select_set_operands(stmt: &SelectStmt) -> Vec<&SelectStmt> {
    let mut operands = Vec::new();
    operands.extend(stmt.larg.as_deref());
    operands.extend(stmt.rarg.as_deref());
    operands
}

pub(crate) fn insert_child_nodes(stmt: &InsertStmt) -> Vec<&Node> {
    let mut children: Vec<&Node> = Vec::new();
    children.extend(stmt.cols.iter());
    children.extend(option_node(stmt.select_stmt.as_deref()));
    children.extend(stmt.returning_list.iter());
    children.extend(with_clause_children(stmt.with_clause.as_ref()));
    children
}

pub(crate) fn update_child_nodes(stmt: &UpdateStmt) -> Vec<&Node> {
    let mut children: Vec<&Node> = Vec::new();
    children.extend(stmt.target_list.iter());
    children.extend(stmt.from_clause.iter());
    children.extend(option_node(stmt.where_clause.as_deref()));
    children.extend(stmt.returning_list.iter());
    children.extend(with_clause_children(stmt.with_clause.as_ref()));
    children
}

pub(crate) fn delete_child_nodes(stmt: &DeleteStmt) -> Vec<&Node> {
    let mut children: Vec<&Node> = Vec::new();
    children.extend(stmt.using_clause.iter());
    children.extend(option_node(stmt.where_clause.as_deref()));
    children.extend(stmt.returning_list.iter());
    children.extend(with_clause_children(stmt.with_clause.as_ref()));
    children
}

pub(crate) fn merge_child_nodes(stmt: &MergeStmt) -> Vec<&Node> {
    let mut children: Vec<&Node> = Vec::new();
    children.extend(option_node(stmt.source_relation.as_deref()));
    children.extend(option_node(stmt.join_condition.as_deref()));
    children.extend(stmt.merge_when_clauses.iter());
    children.extend(stmt.returning_list.iter());
    children.extend(with_clause_children(stmt.with_clause.as_ref()));
    children
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

fn option_node(node: Option<&Node>) -> Vec<&Node> {
    node.into_iter().collect()
}

fn with_clause_children(with_clause: Option<&WithClause>) -> Vec<&Node> {
    match with_clause {
        Some(with_clause) => with_clause.ctes.iter().collect(),
        None => Vec::new(),
    }
}
