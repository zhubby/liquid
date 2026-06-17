# Data Model

Liquid stores application state in PostgreSQL through SQLx migrations embedded
in `liquid-storage`. Target managed databases are separate PostgreSQL databases
owned by users; they are not the same as the Liquid application database.

## Ownership Model

Most tables are scoped by `owner_user_id` and cascade when the owning user is
deleted. The current model has no organization or team table. This keeps
authorization simple: authenticated routes load the current user and every query
filters by that user's id.

## Auth Tables

| Table | Purpose | Important Constraints |
| --- | --- | --- |
| `users` | User profile and password hash. | Unique lower-cased email, non-blank email and display name. |
| `auth_tokens` | Revocable bearer token hashes. | Unique token hash, active lookup index for non-revoked tokens. |

Tokens have `expires_at` and optional `revoked_at`. Logout revokes the current
token.

## Managed Database Tables

| Table | Purpose |
| --- | --- |
| `managed_databases` | Per-user PostgreSQL connection records with encrypted passwords. |
| `user_managed_database_preferences` | Stores the user's current selected managed database. |

Managed database constraints:

- `engine` is currently only `postgres`.
- `ssl_mode` is `disable`, `prefer`, or `require`.
- `port` must be between `1` and `65535`.
- The pair `(owner_user_id, lower(name))` is unique.
- `tags` are stored in the managed database model through later migrations.

The public `ManagedDatabase` DTO includes `has_password` instead of returning
the encrypted secret.

## LLM Provider Settings

| Table | Purpose |
| --- | --- |
| `user_llm_provider_settings` | Per-user OpenAI-compatible provider configuration. |

Stored fields include provider, base URL, model, API mode, encrypted API key, and
`streaming_enabled`. Supported API modes are `chat_completions` and `responses`.

These settings are optional. When absent:

- chat workbench turns become blocked with a provider-not-configured message,
- durable SQL audits can fall back to the process-level audit agent.

## SQL Audit Tables

| Table | Purpose |
| --- | --- |
| `sql_audits` | Durable SQL audit record, lifecycle status, execution state, report JSON, deterministic analysis JSON, and managed database snapshot. |
| `sql_audit_events` | Audit lifecycle event log. |

`sql_audits` stores a snapshot of target connection metadata:

- managed database id,
- name,
- engine,
- host,
- port,
- database name,
- username,
- SSL mode.

Execution checks this snapshot against the current managed database record before
running SQL. If the connection changed, the user must create a new audit.

Supported audit statuses:

| Status | Meaning |
| --- | --- |
| `audited` | Audit completed and is not waiting on approval. |
| `pending_approval` | Write-like SQL has an execution purpose and needs approval. |
| `approved` | A user approved the audit. |
| `rejected` | A user rejected the audit. |
| `blocked` | Deterministic parsing or risk checks prevent normal approval/execution. |
| `executing` | Approved execution has started. |
| `executed` | Execution completed. |
| `execution_failed` | Execution attempted and failed. |

`statement_kind` is nullable and can be `select`, `insert`, `update`, `delete`,
`merge`, `create`, `alter`, `drop`, `truncate`, `security`, `transaction`,
`control`, or `other`.

## Agent Workbench Tables

| Table | Purpose |
| --- | --- |
| `agent_conversations` | Conversation metadata and optional managed database scope. |
| `agent_messages` | User, assistant, tool, and system messages. Timeline-only events are stored as metadata-bearing messages. |
| `agent_turns` | One user request plus assistant response lifecycle. |
| `agent_turn_events` | Ordered event stream for SSE replay and live updates. |
| `agent_actions` | Agent-proposed state changes and their apply/reject lifecycle. |

Turn statuses:

- `queued`
- `running`
- `waiting_for_user`
- `completed`
- `blocked`
- `failed`
- `cancelled`

Action statuses:

- `proposed`
- `applying`
- `applied`
- `rejected`
- `failed`
- `superseded`

`agent_turn_events` has a unique `(turn_id, seq)` constraint so clients can
resume streams with `after_seq`.

## Datapanel Tables

| Table | Purpose |
| --- | --- |
| `datapanels` | One panel per conversation. |
| `datapanel_cards` | Table or chart cards with SQL, layout, and last materialized result. |
| `datapanel_previews` | Public preview slugs for panels. |

Datapanel cards store:

- `managed_database_id`,
- optional `source_action_id`,
- title and optional description,
- `kind` as `table` or `chart`,
- SELECT SQL,
- optional chart config JSON,
- layout JSON,
- result JSON.

The frontend renders layout using a 12-column grid. Card layout is stored as
`x`, `y`, `w`, and `h`.

## Database Diagram Tables

| Table | Purpose |
| --- | --- |
| `database_diagrams` | User-owned database design records with title, optional description, and document JSON. |

Database diagram documents store canvas-ready database design metadata:

- `tables` with schema, position, columns, and indexes,
- `relationships` with source/target endpoints, cardinality, and referential
  actions,
- `enums`,
- optional freeform `notes` and `areas`.

The table is intentionally decoupled from `managed_databases`. Chat-generated
database designs record the source database in the agent action payload and the
diagram title/description, but the persisted diagram record does not keep a
foreign key to the managed database.

## Database Backup Tables

| Table | Purpose |
| --- | --- |
| `database_backups` | Queued/running/succeeded/failed/deleted PostgreSQL dump jobs and local or S3 storage metadata. |
| `database_restore_jobs` | Queued/running/succeeded/failed/deleted restore jobs tied to a backup and target database snapshot. |

Backup format is currently only `postgres_custom`, created by `pg_dump
--format=custom`. Backup storage metadata records `storage_kind` as `local` or
`s3`. Local backups store `local_path`; S3 backups store bucket, key, optional
version id, optional ETag, size, and SHA-256 checksum.

Both backup and restore records keep source or target managed database snapshots
so job history remains meaningful even if the managed database record later
changes.

## Generated Contracts

Types in `liquid-core` are annotated with `#[derive(TS)]` and `#[ts(export)]`.
Running:

```bash
cargo test -p liquid-core
```

regenerates TypeScript files under `liquid-ui/lib/generated/api-types`.

Do not edit generated TypeScript files by hand. Change the Rust DTO, run the
contract generation test, and commit both the Rust and generated TypeScript
changes.
