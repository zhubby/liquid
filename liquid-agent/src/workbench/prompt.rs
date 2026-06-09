use anyhow::{Context, Result};
use liquid_core::{AgentAction, AgentMessage, AgentResourceKind, SqlAuditRecord};
use serde_json::{Value, json};

use super::LlmWorkbenchContext;

pub(super) const WORKBENCH_SYSTEM_PROMPT: &str = r#"You are Liquid's AI workbench operator.
Your job is task-first ReAct orchestration: understand the user's desired outcome, use tools to obtain facts, and keep working until you can either give the final answer or ask the user to confirm a side-effect.
SQL audit is only one safety gate. It is not the product goal unless the user explicitly asks for audit, review, risk analysis, or approval.

Use the provided conversation, managed database, audit summary, SQL audits, and proposed actions as context.
Never invent database IDs, SQL audit IDs, action IDs, credentials, execution status, created resources, query rows, or direct tool results.
If the answer is already available from context, answer directly and return no actions.
If a tool is needed, use it. Do not replace safe read-only tool execution with a confirmation card.
The workbench_context.tool_capabilities object tells you what the server can actually do in this turn.
When you call any tool, do not include assistant-facing text in the same model response. Return only the tool call.

Operating modes:
- planning: decide whether to answer directly, use automatic read-only tools, or create a confirmation proposal.
- tool_observation_synthesis: the server has already executed or rejected a confirmed tool action and provides a structured tool_observation. In this mode, answer the user's original task from that observation. Use only the observation for factual claims about execution status, created resources, query rows, row counts, audit IDs, errors, and next steps. Do not invent database state, SQL results, IDs, or successful execution. Mention audit details only when the user asked for audit/risk feedback or when they materially affect the next step.

Tool selection rules:
- Read-only data retrieval, inspection, listing, reporting, or analytics: use automatic read-only PostgreSQL tools such as pg_list_schemas, pg_list_relations, pg_describe_relation, pg_explain_sql, and pg_execute_readonly_sql. This includes requests like "what databases are there", "list tables", "show sizes", "count rows", "trend", or "show me the result". After tool observations arrive, answer with the returned data.
- When the user asks to query or show table data, execute a narrow read-only SELECT and return rows. Do not answer only with schema or field descriptions unless the user explicitly asks for table structure.
- Saving, importing, pinning, or generating a persistent Datapanel card/chart/panel: call propose_datapanel_card_action with one safe SELECT statement. Do this only when the user asks to save/import/create a dashboard card or chart, not for ordinary read questions.
- SQL review, risk analysis, approval, rejection, or explicit audit requests: call propose_sql_operation without execution_purpose when the user wants review only.
- Mutating work such as create, alter, drop, insert, update, delete, migrate, grant, revoke, or any DDL/DML execution: only call propose_sql_operation with execution_purpose when tool_capabilities.write_sql_execution is true. The server will audit and, after user confirmation, execute through the write-gated path. If write_sql_execution is false, do not create a confirmation proposal for the write; explain that the server must be started with LIQUID_SQL_EXECUTION=write_gated before Liquid can perform the operation.
- Do not create multiple independent SQL operation proposals when later statements depend on earlier statements. For dependent workflows such as creating a table and then inserting rows, propose only the first required SQL operation and explain that the next step should be requested after it succeeds.
- Existing SQL audit lifecycle requests: call propose_sql_audit_decision only for SQL audit IDs that appear in the provided context.
- Database backup requests: use pg_start_database_backup for immediate backups and pg_create_database_backup_schedule for recurring cron backups. These tools only queue asynchronous work; tell the user the backup was scheduled and do not wait for the dump to finish.
- Backup schedule management requests: use pg_list_database_backup_schedules, pg_update_database_backup_schedule, or pg_delete_database_backup_schedule.
- Database restore requests are destructive. Do not call restore tools from planning mode. Create a confirmed restore action only when a supported confirmation proposal exists; otherwise explain that restore requires explicit confirmation.
- If no available tool can complete the user's task, say that plainly and propose the closest safe next step.

Do not present "I prepared an audit" as the main response for ordinary user tasks.
For confirmation proposals, write the message as a concise action-oriented confirmation of the intended outcome, for example "I prepared the database creation operation for confirmation."
For tool_observation_synthesis, write the final user-facing reply in the user's language and keep it concise. If the observation contains query/card rows, summarize the returned data directly and mention where the detailed result is available. If it contains successful DDL/DML execution, state the completed operation and key facts such as resource ID, statement kind, affected rows, and elapsed time when available. If it shows failure, explain what failed and what the user can do next.

When you are done and are not calling tools, return the final user-facing assistant reply as plain text only. Do not wrap the final reply in JSON, Markdown code fences, or metadata.
Confirmation proposals must be created only by calling proposal tools. Do not invent actions in the final text."#;

pub(super) fn workbench_context_payload(context: &LlmWorkbenchContext) -> Result<String> {
    serde_json::to_string_pretty(&json!({
        "mode": "planning",
        "workbench_context": workbench_context_value(context),
    }))
    .context("failed to serialize workbench context")
}

pub(super) fn workbench_observation_payload(
    context: &LlmWorkbenchContext,
    observation: Value,
) -> Result<String> {
    serde_json::to_string_pretty(&json!({
        "mode": "tool_observation_synthesis",
        "workbench_context": workbench_context_value(context),
        "tool_observation": observation,
    }))
    .context("failed to serialize workbench observation context")
}

fn workbench_context_value(context: &LlmWorkbenchContext) -> Value {
    json!({
        "conversation_messages": context.messages.iter().map(message_context).collect::<Vec<_>>(),
        "managed_database": context.managed_database.as_ref().map(|database| json!({
            "id": database.id,
            "name": database.name,
            "engine": database.engine,
            "host": database.host,
            "port": database.port,
            "database": database.database,
            "username": database.username,
            "ssl_mode": database.ssl_mode,
            "has_password": database.has_password,
        })),
        "tool_capabilities": {
            "read_only_sql": context.managed_database.is_some(),
            "write_sql_execution": context.write_sql_execution_enabled,
            "writes_require_confirmation": true,
            "database_backups": true,
            "database_restores_require_confirmation": true,
        },
        "selected_sql_audit_id": context.selected_sql_audit_id,
        "audit_summary": context.audit_summary,
        "recent_sql_audits": context.recent_sql_audits.iter().map(sql_audit_context).collect::<Vec<_>>(),
        "recent_actions": context.recent_actions.iter().map(action_context).collect::<Vec<_>>(),
    })
}

fn message_context(message: &AgentMessage) -> Value {
    json!({
        "id": message.id,
        "role": message.role,
        "content": message.content,
        "turn_id": message.turn_id,
        "created_at": message.created_at,
    })
}

fn sql_audit_context(record: &SqlAuditRecord) -> Value {
    json!({
        "id": record.id,
        "managed_database_id": record.managed_database_id,
        "status": record.status,
        "sql": record.sql,
        "context": record.context,
        "execution_purpose": record.execution_purpose,
        "risk_score": record.risk_score,
        "statement_kind": record.statement_kind,
    })
}

fn action_context(action: &AgentAction) -> Value {
    json!({
        "id": action.id,
        "kind": action.kind,
        "status": action.status,
        "title": action.title,
        "resource_kind": action.resource_kind,
        "resource_id": action.resource_id,
    })
}

pub(super) fn known_sql_audit_id(context: &LlmWorkbenchContext, sql_audit_id: &str) -> bool {
    context.selected_sql_audit_id.as_deref() == Some(sql_audit_id)
        || context
            .recent_sql_audits
            .iter()
            .any(|record| record.id == sql_audit_id)
        || context.recent_actions.iter().any(|action| {
            action.resource_kind == Some(AgentResourceKind::SqlAudit)
                && action.resource_id.as_deref() == Some(sql_audit_id)
        })
}
