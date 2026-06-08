# Product Design

Liquid is designed as an operational database workspace, not a reporting-only
dashboard. The product goal is to make database work reviewable: every managed
database, SQL audit, AI suggestion, execution result, and datapanel card is
stored as durable application state.

## Product Principles

- Make the selected database explicit. Most workspace actions are scoped to one
  managed PostgreSQL database record.
- Treat SQL as untrusted input. SQL is parsed and classified before it is shown
  as safe, saved as a panel card, or executed.
- Separate suggestion from mutation. The AI agent can propose actions, but
  state-changing actions are persisted as `agent_actions` and applied only after
  user confirmation.
- Preserve context. Chat messages, tool activity, audit records, query results,
  and datapanel cards remain attached to the conversation that produced them.
- Keep frontend contracts typed. Browser code imports generated TypeScript types
  from Rust DTOs instead of hand-written API shapes.

## Users and Workspaces

The current product model is single-user ownership per record:

- A `users` row owns managed database records, conversations, SQL audits,
  datapanels, backup jobs, and LLM provider settings.
- Authentication uses bearer tokens. Tokens are stored only as hashes and can be
  revoked on logout.
- A user may register multiple managed databases, but the dashboard requires one
  selected database before entering the main workspace.

The workspace is conversation-based. Each selected database can have many chat
conversations. Each conversation has exactly one datapanel, created lazily when
the frontend requests it.

## Primary User Flow

1. The user logs in or registers.
2. The frontend loads `/api/v1/auth/me` and
   `/api/v1/managed-databases/current` from the saved token.
3. If no current database is selected, the user creates or selects a managed
   database.
4. The dashboard opens the most recent conversation for that database or creates
   a default one.
5. The user can work in chat mode or SQL mode on the left pane.
6. The datapanel on the right pane reflects cards saved from SQL results or
   agent actions.

## Dashboard Layout

The Next.js app renders three major states:

| State | Component | Purpose |
| --- | --- | --- |
| Auth screen | `LiquidApp` auth branch | Login and registration, theme and language shortcuts. |
| Database picker | `ManagedDatabasePicker` | Manage target PostgreSQL connection records and choose the active database. |
| Workspace | `AuditDashboard` | Split-pane AI/SQL workspace plus datapanel. |

The workspace split is intentionally dense. The left pane is for interaction and
the right pane is for persistent outputs. Users can resize the panes, create or
delete conversations, rename conversations, and save query results into the
datapanel.

## Chat Mode

Chat mode is the agent-native interface. A user message creates an `agent_turn`
and starts a background task in the API process. The frontend streams turn events
from `/api/v1/chat/turns/{turn_id}/stream`.

The agent receives:

- recent conversation messages,
- the selected managed database,
- current dashboard context,
- recent SQL audits,
- recent agent actions,
- the mock or real audit summary,
- read-only PostgreSQL tools when a managed database is selected.

The agent can return normal assistant text and proposed actions. Proposed actions
are rendered for the user and stored with `status = proposed`. Applying an action
moves it to `applying`, performs the backend operation, then marks it as
`applied` or `failed`.

## SQL Mode

SQL mode is direct execution for the selected managed database. It is not the
same as the audited write-execution flow:

- SQL mode requires exactly one PostgreSQL statement.
- Empty SQL, parse failures, transaction statements, and control statements are
  rejected.
- SELECT statements are materialized through the datapanel query path and can be
  saved as table cards.
- INSERT, UPDATE, DELETE, or MERGE statements with `RETURNING` are returned as a
  query-result table but are not saveable as datapanel cards.
- Other accepted mutation statements return a summary with affected rows and
  elapsed time.

SQL mode runs statements inside a transaction with `statement_timeout = '5s'`.
Mutation statements commit when they succeed.

## SQL Audit Mode

SQL audit mode is the review workflow for SQL that may need approval:

- A SQL audit stores the submitted SQL, optional schema and context, optional
  execution purpose, deterministic parser analysis, model report, status, risk
  score, and a snapshot of the managed database connection metadata.
- SELECT audits are considered read-only and can materialize a limited query
  result.
- Write statements with an execution purpose become `pending_approval` unless
  deterministic analysis blocks them.
- Critical deterministic findings block the audit.
- Approved write execution requires `LIQUID_SQL_EXECUTION=write_gated`.

The snapshot fields on an audit prevent execution after the managed database
connection record changes.

## Datapanels

A datapanel is the persistent output surface for a conversation. It contains
cards with:

- a title and optional description,
- a managed database id,
- table or chart kind,
- the SELECT SQL that produced the card,
- a layout rectangle for the 12-column grid,
- the last materialized query result.

Datapanel card SQL must be one SELECT statement. Liquid strips a trailing
semicolon, rejects row-locking SELECTs, runs the query in a read-only
transaction, applies a 5 second statement timeout, and clamps result size.

Cards can be refreshed, renamed, deleted, rearranged, exported as JSON, or shared
through a public preview slug.

## Action Model

Agent actions are the boundary between AI planning and application mutation.
Supported action kinds are:

- `create_sql_audit`
- `create_datapanel_card`
- `approve_sql_audit`
- `reject_sql_audit`
- `execute_sql_audit`
- `create_managed_database`
- `update_managed_database`
- `delete_managed_database`
- `start_database_backup`
- `start_database_restore`

Action statuses are `proposed`, `applying`, `applied`, `rejected`, `failed`, and
`superseded`. Applying SQL-related actions respects ordering constraints so a
later action cannot skip a required earlier audit or approval action in the same
turn.

Current workbench action application supports SQL audit actions and datapanel
card creation. Managed database changes and backup/restore action kinds are
defined in the shared enum and storage constraints, but the workbench API returns
a conflict for those action kinds until product flows are wired.

## Current Boundaries

- PostgreSQL is the only managed database engine.
- The dashboard has no separate organization, team, or role model yet.
- The API exposes direct routes for user, database, chat, SQL audit, settings,
  and datapanel operations. Backup and restore are implemented as storage and
  worker foundations, with tool definitions present, but are not wired as
  first-class REST routes or supported workbench actions yet.
- The backend falls back to a mock SQL audit agent when process-level OpenAI
  credentials are missing. Chat mode requires user-level LLM provider settings.
