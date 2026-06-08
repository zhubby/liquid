# Liquid

Liquid is an AI audit and observability layer for PostgreSQL. It gives a team a
single workspace for registering target databases, asking an AI agent for help,
auditing SQL before it is run, executing approved statements, and turning safe
query results into a persistent datapanel.

The repository contains one deployable backend binary and one dashboard
frontend:

- The backend is a Rust 2024 workspace exposed by the `liquid` CLI.
- The API server is built with Axum and stores Liquid application data in
  PostgreSQL through SQLx.
- SQL analysis is deterministic first: Liquid parses PostgreSQL with `pg_query`,
  classifies statement kind, and applies local safety rules before agent output
  is persisted.
- The AI layer is provider-agnostic inside Liquid and currently speaks
  OpenAI-compatible Chat Completions and Responses protocols.
- The frontend is a Bun-managed Next.js application using TypeScript, Tailwind,
  shadcn/ui-style primitives, lucide-react, react-grid-layout, and Recharts.
- Shared API contracts live in `liquid-core` and are exported to TypeScript with
  `ts-rs`.

## Product Surface

Liquid is organized around a database workspace:

1. A user registers or logs in.
2. The user creates one or more managed PostgreSQL database records.
3. The user selects a managed database and enters the workspace.
4. The left pane provides chat and direct SQL execution modes.
5. The right pane shows a datapanel for the active conversation.
6. Agent suggestions that change state are represented as explicit actions and
   require user confirmation.

The core product capabilities are:

| Capability | What Liquid Does |
| --- | --- |
| Authentication | Stores users, password hashes, bearer token hashes, and revocation state. |
| Managed databases | Stores per-user PostgreSQL connection records with encrypted passwords and lazy connection pools. |
| SQL audit | Parses and classifies SQL, collects deterministic findings, optionally uses an LLM tool loop, and stores a durable audit record. |
| Approval and execution | Lets SELECT audits materialize read-only results and gates write execution behind audit approval and `LIQUID_SQL_EXECUTION=write_gated`. |
| AI workspace | Streams agent turns over SSE, records tool activity, and persists proposed actions. |
| Datapanels | Saves table or chart cards backed by SELECT queries, refreshes results, exports panels, and exposes public preview slugs. |
| Database operations | Provides storage, worker, and internal tool foundations for S3-backed PostgreSQL dump and restore jobs when backup storage is configured. These jobs are not exposed as first-class REST routes yet. |

## Repository Map

| Path | Responsibility |
| --- | --- |
| `liquid-cli` | `liquid` binary, Clap command parsing, startup logging, config loading, migration and server commands. |
| `liquid-api` | Axum routes, API state wiring, CORS, managed pool construction, background worker startup. |
| `liquid-config` | TOML plus environment configuration, defaults, validation, mode parsing. |
| `liquid-core` | Shared domain and transport types exported to TypeScript. |
| `liquid-agent` | SQL audit agents, workbench agent, tool registry, PostgreSQL tools, backup/restore worker. |
| `liquid-llm` | OpenAI-compatible client abstraction for synchronous and streaming LLM calls. |
| `liquid-sql` | PostgreSQL parsing, statement classification, metadata shape, and deterministic risk rules. |
| `liquid-storage` | SQLx storage implementation, migrations, auth, encrypted secrets, managed pools. |
| `liquid-ui` | Next.js dashboard and generated API type imports. |
| `docs` | mdBook documentation. |

## How to Read These Docs

- Start with [Product Design](./product-design.md) to understand the user model,
  workspace layout, and state-changing action model.
- Read [System Architecture](./system-architecture.md) for crate boundaries and
  runtime components.
- Read [Core Flows](./core-flows.md) for the end-to-end behavior of auth,
  managed databases, chat turns, SQL audits, datapanel cards, and backup jobs.
- Use [API Reference](./api-reference.md), [Data Model](./data-model.md), and
  [SQL Safety Model](./sql-safety-model.md) when changing contracts or backend
  behavior.
- Use [Configuration and Operations](./configuration-and-operations.md) and
  [Development](./development.md) when running Liquid locally or deploying the
  API.
