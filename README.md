# Liquid

Liquid is a Rust and Next.js SQL AI audit dashboard. It provides an Axum API for
authentication, managed database connection records, and SQL audit summaries,
plus a Bun-managed Next.js frontend for operational datapanel workflows.

## Features

- Rust 2024 workspace with focused `liquid-*` crates.
- Axum API server exposed by the `liquid` CLI.
- Postgres-backed users, auth tokens, and managed database records.
- SQL audit agent boundary with mock and OpenAI-compatible implementations.
- Next.js dashboard built with TypeScript, Tailwind, shadcn/ui-style
  primitives, lucide-react, and Recharts.
- mdBook documentation under `docs/`.

## Repository Layout

| Path | Purpose |
| --- | --- |
| `liquid-cli` | CLI entry point and process startup. |
| `liquid-api` | Axum routes and HTTP server composition. |
| `liquid-config` | TOML and environment-backed configuration. |
| `liquid-core` | Shared domain and API transport types. |
| `liquid-agent` | SQL audit agent traits, prompts, tools, and implementations. |
| `liquid-llm` | OpenAI-compatible LLM client abstractions. |
| `liquid-sql` | SQL parsing, analysis, metadata, and risk rules. |
| `liquid-storage` | Postgres storage, SQLx migrations, auth, and managed databases. |
| `liquid-ui` | Bun-managed Next.js dashboard. |
| `docs` | mdBook documentation. |

## Requirements

- Rust stable with Rust 2024 support.
- Bun for frontend development.
- PostgreSQL for the Liquid application database.
- Docker, optional for containerized API runs.
- mdBook, optional for documentation preview.

## Quick Start

Start a local Postgres database if you do not already have one:

```bash
docker run --name liquid-postgres \
  -e POSTGRES_PASSWORD=postgres \
  -e POSTGRES_DB=liquid \
  -p 5432:5432 \
  -d postgres:16
```

Run the backend API:

```bash
cargo run -p liquid-cli -- migrate
cargo run -p liquid-cli -- server
```

The API defaults to `127.0.0.1:3001` and exposes `GET /healthz`.

Run the frontend in another terminal:

```bash
cd liquid-ui
bun install
bun run dev
```

The dashboard runs on `http://localhost:3000` and reads the API from
`NEXT_PUBLIC_API_BASE_URL`, defaulting to `http://localhost:3001`.

## Configuration

When no config path is provided, the CLI reads `~/.liquid/config.toml` and
creates it with development defaults if it is missing. You can pass an explicit
config file:

```bash
cargo run -p liquid-cli -- server --config liquid.toml
```

Environment variables override config file values.

| Variable | Default | Purpose |
| --- | --- | --- |
| `LIQUID_API_ADDR` | `127.0.0.1:3001` | API bind address. |
| `LIQUID_CORS_ORIGIN` | `http://localhost:3000` | Allowed browser origin. |
| `LIQUID_DATABASE_URL` or `DATABASE_URL` | `postgres://postgres:postgres@localhost:5432/liquid` | Liquid application database. |
| `LIQUID_DATABASE_MAX_CONNECTIONS` | `5` | Postgres connection pool size. |
| `LIQUID_DATABASE_AUTO_MIGRATE` | `true` | Run migrations at API startup. |
| `LIQUID_AUTH_TOKEN_TTL_SECONDS` | `604800` | Auth token lifetime. |
| `LIQUID_ENCRYPTION_KEY` | development key | Secret used to encrypt managed database passwords. |
| `OPENAI_API_KEY` | unset | Enables the OpenAI-compatible SQL audit agent when paired with `OPENAI_MODEL`. |
| `OPENAI_MODEL` | unset | Model name for the OpenAI-compatible SQL audit agent. |
| `OPENAI_BASE_URL` | `https://api.openai.com` | OpenAI-compatible provider base URL. |
| `OPENAI_API_MODE` | `chat_completions` | `chat_completions` or `responses`. |
| `LIQUID_SQL_METADATA` | `auto` | `auto`, `off`, or `required`. |
| `LIQUID_SQL_EXECUTION` | `readonly` | `off`, `readonly`, or `write_gated`. |
| `LIQUID_SQL_MANAGED_POOL_MAX_CONNECTIONS` | `2` | Maximum connections per managed database pool. |
| `LIQUID_SQL_MANAGED_POOL_IDLE_TTL_SECONDS` | `600` | Close an unused managed database pool after this many seconds. |
| `LIQUID_SQL_MANAGED_POOL_REAP_INTERVAL_SECONDS` | `60` | Background interval for managed database pool cleanup. |
| `LIQUID_SQL_MANAGED_POOL_ACQUIRE_TIMEOUT_SECONDS` | `10` | SQLx acquire timeout for managed database pools. |
| `LIQUID_SQLX_LOGS` | `false` | Set to `true` to enable pretty SQLx query logs. |
| `LIQUID_BACKUP_S3_BUCKET` | unset | S3 bucket for managed database backup files; backup worker is disabled when unset. |
| `LIQUID_BACKUP_S3_PREFIX` | `liquid/database-backups` | S3 key prefix for backup objects. |
| `LIQUID_BACKUP_S3_REGION` | `us-east-1` | S3 signing region. |
| `LIQUID_BACKUP_S3_ENDPOINT` | unset | Optional S3-compatible endpoint. |
| `LIQUID_BACKUP_S3_PATH_STYLE` | `false` | Use path-style S3 URLs, useful for S3-compatible services. |
| `LIQUID_BACKUP_WORK_DIR` | `/tmp/liquid-backups` | Local temporary directory for `pg_dump` and `pg_restore` files. |
| `LIQUID_BACKUP_WORKER_CONCURRENCY` | `1` | Number of database backup/restore worker tasks. |

Managed database SQL audit tools honor `LIQUID_SQL_EXECUTION`: `off` disables
execution tools, `readonly` exposes read-only execution, and `write_gated`
exposes gated write execution for audited, user-approved statements.
SQLx query logs are disabled by default; set `LIQUID_SQLX_LOGS=true` only while
debugging local database execution.

Set a real `LIQUID_ENCRYPTION_KEY` before storing managed database passwords
outside local development.

Database backup storage uses S3-compatible object storage. When
`LIQUID_BACKUP_S3_BUCKET` is set, the API starts a background worker that uses
`pg_dump`, `pg_restore`, and AWS credentials from the process environment.

## Docker

Build the API image from the repository root:

```bash
docker build -t liquid .
```

Run the API container:

```bash
docker run --rm \
  -p 3001:3001 \
  -e LIQUID_DATABASE_URL=postgres://postgres:postgres@host.docker.internal:5432/liquid \
  -e LIQUID_ENCRYPTION_KEY=replace-with-a-secret-key \
  liquid
```

The image runs `liquid server` and binds to `0.0.0.0:3001` inside the container.
It does not serve the Next.js dashboard; run `liquid-ui` separately or deploy it
as a separate frontend service.

## Development Commands

Run Rust checks from the repository root:

```bash
cargo check --workspace
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

Run frontend checks from `liquid-ui/`:

```bash
bun run lint
bun run build
```

Preview the docs:

```bash
mdbook serve docs
```

## License

Liquid is licensed under the MIT License. See `LICENSE` for details.
