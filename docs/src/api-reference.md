# API Reference

The backend serves JSON routes under `/api/v1` and a health route at `/healthz`.
Authenticated routes require:

```text
Authorization: Bearer <token>
```

Errors use a common JSON shape:

```json
{
  "error": "message",
  "details": {}
}
```

`details` is omitted unless a route has structured conflict diagnostics.

## Health

| Method | Path | Auth | Purpose |
| --- | --- | --- | --- |
| `GET` | `/healthz` | No | Returns `{ "status": "ok", "service": "liquid-api" }`. |

Health checks do not require a database connection.

## Authentication

| Method | Path | Auth | Purpose |
| --- | --- | --- | --- |
| `POST` | `/api/v1/auth/register` | No | Create a user and return `AuthResponse`. |
| `POST` | `/api/v1/auth/login` | No | Verify credentials and return `AuthResponse`. |
| `POST` | `/api/v1/auth/logout` | Yes | Revoke the current bearer token. |
| `GET` | `/api/v1/auth/me` | Yes | Return the current user. |
| `PATCH` | `/api/v1/auth/me` | Yes | Update current user profile fields. |
| `PATCH` | `/api/v1/auth/password` | Yes | Update the current user's password. |

## Managed Databases

| Method | Path | Auth | Purpose |
| --- | --- | --- | --- |
| `GET` | `/api/v1/managed-databases/current` | Yes | Return the current selected database, if any. |
| `PUT` | `/api/v1/managed-databases/current` | Yes | Set the current selected database. |
| `DELETE` | `/api/v1/managed-databases/current` | Yes | Clear the current selected database. |
| `GET` | `/api/v1/managed-databases` | Yes | List the user's managed database records. |
| `POST` | `/api/v1/managed-databases` | Yes | Create a managed PostgreSQL connection record. |
| `PATCH` | `/api/v1/managed-databases/{id}` | Yes | Update connection metadata, tags, SSL mode, or password. |
| `DELETE` | `/api/v1/managed-databases/{id}` | Yes | Delete a managed database record. |
| `POST` | `/api/v1/managed-databases/{id}/test-connection` | Yes | Build a target database pool and run `select 1`. |
| `POST` | `/api/v1/managed-databases/{id}/audit-sql` | Yes | Run an immediate SQL audit report without creating a durable audit record. |

Updating or deleting a managed database invalidates its cached target pool.

## SQL Audits

| Method | Path | Auth | Purpose |
| --- | --- | --- | --- |
| `POST` | `/api/v1/managed-databases/{id}/sql-audits` | Yes | Create a durable SQL audit for a managed database. |
| `GET` | `/api/v1/sql-audits` | Yes | List SQL audits with optional filters and pagination headers. |
| `GET` | `/api/v1/sql-audits/{id}` | Yes | Fetch one SQL audit. |
| `POST` | `/api/v1/sql-audits/{id}/approve` | Yes | Approve an audit with an optional comment. |
| `POST` | `/api/v1/sql-audits/{id}/reject` | Yes | Reject an audit with an optional comment. |
| `POST` | `/api/v1/sql-audits/{id}/execute` | Yes | Execute an approved write audit or materialize a SELECT audit. |

List query parameters:

| Parameter | Meaning |
| --- | --- |
| `managed_database_id` | Restrict to one managed database. |
| `status` | Filter by full `SqlAuditStatus`. |
| `audit_status` | Filter by lifecycle status. |
| `execution_status` | Filter by execution status. |
| `created_from` | RFC3339 lower bound. |
| `created_to` | RFC3339 upper bound; must be after `created_from`. |
| `page` | 1-based page number. |
| `page_size` | Must be `10`, `20`, `50`, or `100`. |
| `limit` | Legacy limit, clamped to `1..100` when `page_size` is absent. |

List responses include `X-Total-Count`, `X-Page`, and `X-Page-Size` headers.

`SqlAuditExecutionResult` includes rollback metadata when a write execution has
completed:

```json
{
  "statement_kind": "update",
  "affected_rows": 3,
  "elapsed_ms": 42,
  "risk_floor": 40,
  "findings": [],
  "rollback": {
    "status": "generated",
    "sql": "with rollback_rows as (...) ...",
    "generated_at": "2026-06-09T10:00:00Z"
  }
}
```

`rollback.status` is `generated`, `unsupported`, or `failed`. Unsupported and
failed plans include `reason` instead of `sql`. Older records may omit
`rollback` because it is stored in the existing `execution_result` JSONB value.

## Chat and Workbench

| Method | Path | Auth | Purpose |
| --- | --- | --- | --- |
| `GET` | `/api/v1/chat/conversations` | Yes | List conversations, optionally by `managed_database_id`. |
| `POST` | `/api/v1/chat/conversations` | Yes | Create a conversation. |
| `GET` | `/api/v1/chat/conversations/{conversation_id}` | Yes | Fetch a conversation. |
| `PATCH` | `/api/v1/chat/conversations/{conversation_id}` | Yes | Rename a conversation. |
| `DELETE` | `/api/v1/chat/conversations/{conversation_id}` | Yes | Delete a conversation. |
| `GET` | `/api/v1/chat/conversations/{conversation_id}/messages` | Yes | List non-timeline chat messages. |
| `GET` | `/api/v1/chat/conversations/{conversation_id}/actions` | Yes | List actions for a conversation. |
| `POST` | `/api/v1/chat/conversations/{conversation_id}/turns` | Yes | Create and start an AI chat turn. |
| `POST` | `/api/v1/chat/conversations/{conversation_id}/sql-executions` | Yes | Execute direct SQL mode statement. |
| `GET` | `/api/v1/chat/turns/{turn_id}/stream` | Yes | Stream chat turn events over SSE. |
| `POST` | `/api/v1/chat/turns/{turn_id}/cancel` | Yes | Mark a turn cancelled. |
| `POST` | `/api/v1/chat/actions/{action_id}/apply` | Yes | Apply a proposed or failed action. |
| `POST` | `/api/v1/chat/actions/{action_id}/reject` | Yes | Reject a proposed action. |

Conversation query parameters:

| Route | Parameter | Meaning |
| --- | --- | --- |
| `GET /chat/conversations` | `managed_database_id` | Restrict to conversations scoped to a selected database. |
| `GET /chat/conversations` | `limit` | Max conversations, default `50`. |
| `GET /messages` | `limit` | Max messages, default `100`. |
| `GET /messages` | `before` | Cursor before a message id. |
| `GET /actions` | `status` | Filter by action status. |
| `GET /turns/{turn_id}/stream` | `after_seq` | Resume after an event sequence number. |

The stream endpoint emits typed `ChatStreamEvent` JSON frames and `ping` keepalive
frames.

SQL mode assistant message parts may include rollback metadata:

- `query_result_table.rollback` for write statements with `RETURNING`,
- `sql_execution_summary.rollback` for write statements summarized by affected
  row count.

SELECT query results omit `rollback`. Unsupported rollback generation does not
change whether the SQL statement commits; it is reported with
`status: "unsupported"` and a reason.

## Datapanels

| Method | Path | Auth | Purpose |
| --- | --- | --- | --- |
| `GET` | `/api/v1/chat/conversations/{conversation_id}/datapanel` | Yes | Get or create the conversation datapanel. |
| `PATCH` | `/api/v1/chat/conversations/{conversation_id}/datapanel` | Yes | Update datapanel title or description. |
| `POST` | `/api/v1/chat/conversations/{conversation_id}/datapanel/cards` | Yes | Save a table card from an existing query result. |
| `PATCH` | `/api/v1/datapanels/{panel_id}/layout` | Yes | Persist card layout updates. |
| `PATCH` | `/api/v1/datapanels/{panel_id}/cards/{card_id}` | Yes | Update card title or description. |
| `DELETE` | `/api/v1/datapanels/{panel_id}/cards/{card_id}` | Yes | Delete a card. |
| `POST` | `/api/v1/datapanels/{panel_id}/cards/{card_id}/refresh` | Yes | Re-run the card SELECT and update its result. |
| `GET` | `/api/v1/datapanels/{panel_id}/export` | Yes | Export a panel with cards and current results. |
| `POST` | `/api/v1/datapanels/{panel_id}/preview` | Yes | Create or return a public preview slug. |
| `GET` | `/api/v1/datapanel-previews/{slug}` | No | Fetch a public preview by slug. |

Datapanel SQL constraints:

- exactly one PostgreSQL statement,
- statement kind must be SELECT,
- row-locking SELECTs are rejected,
- result limit is clamped to `1..1000`,
- materialization runs in a read-only transaction with a 5 second statement
  timeout.

## Settings

| Method | Path | Auth | Purpose |
| --- | --- | --- | --- |
| `GET` | `/api/v1/settings/llm-provider` | Yes | Return user-level provider settings. |
| `PUT` | `/api/v1/settings/llm-provider` | Yes | Upsert OpenAI-compatible provider settings. |

User-level provider settings are used by chat workbench turns and by SQL audit
creation when present. API keys are encrypted in storage and public responses
only report whether a key exists.

## Audit Summary

| Method | Path | Auth | Purpose |
| --- | --- | --- | --- |
| `GET` | `/api/v1/audit/summary` | Yes | Return `AuditSummary` from the configured process-level audit agent. |

With the mock agent this route returns mock dashboard summary data.
