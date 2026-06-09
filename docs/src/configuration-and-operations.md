# Configuration and Operations

The `liquid` CLI reads configuration from a TOML file plus environment
variables. Environment variables override file values.

## Commands

Run from the repository root:

```bash
cargo run -p liquid-cli -- server
cargo run -p liquid-cli -- migrate
cargo run -p liquid-cli -- version
```

When `--config` is omitted, the CLI uses:

```text
~/.liquid/config.toml
```

If that file is missing, the CLI creates it with development defaults before
loading it.

Use an explicit file with:

```bash
cargo run -p liquid-cli -- server --config liquid.toml
cargo run -p liquid-cli -- migrate --config liquid.toml
```

## Default TOML Shape

```toml
[api]
addr = "127.0.0.1:3001"
cors_origin = "http://localhost:3000"

[database]
url = "postgres://postgres:postgres@localhost:5432/liquid"
max_connections = 5
auto_migrate = true

[auth]
token_ttl_seconds = 604800

[security]
encryption_key = "liquid-development-encryption-key-change-me"

[llm]
base_url = "https://api.openai.com"
api_mode = "chat_completions"

[workbench]
max_tool_rounds = 10
# max_output_tokens = 4000

[sql]
metadata = "auto"
execution = "readonly"
managed_pool_max_connections = 2
managed_pool_idle_ttl_seconds = 600
managed_pool_reap_interval_seconds = 60
managed_pool_acquire_timeout_seconds = 10

[backup]
s3_prefix = "liquid/database-backups"
s3_region = "us-east-1"
s3_path_style = false
work_dir = "~/.liquid/backup"
worker_concurrency = 1
```

Set a real `LIQUID_ENCRYPTION_KEY` outside local development. It protects managed
database passwords and user-level LLM API keys.

## Environment Variables

| Variable | Default | Purpose |
| --- | --- | --- |
| `LIQUID_API_ADDR` | `127.0.0.1:3001` | API bind address. |
| `LIQUID_CORS_ORIGIN` | `http://localhost:3000` | Allowed browser origin. |
| `LIQUID_DATABASE_URL` | `postgres://postgres:postgres@localhost:5432/liquid` | Liquid application database URL. |
| `DATABASE_URL` | same as above | Fallback application database URL when `LIQUID_DATABASE_URL` is unset. |
| `LIQUID_DATABASE_MAX_CONNECTIONS` | `5` | SQLx pool size for the application database. |
| `LIQUID_DATABASE_AUTO_MIGRATE` | `true` | Run embedded SQLx migrations at API startup. |
| `LIQUID_AUTH_TOKEN_TTL_SECONDS` | `604800` | Bearer token lifetime. |
| `LIQUID_ENCRYPTION_KEY` | development key | Secret used for encrypted managed database passwords and provider keys. |
| `OPENAI_API_KEY` | unset | Process-level OpenAI-compatible API key for fallback SQL audit agent. |
| `OPENAI_MODEL` | unset | Process-level model name for fallback SQL audit agent. |
| `OPENAI_BASE_URL` | `https://api.openai.com` | Process-level provider base URL or complete endpoint URL. |
| `OPENAI_API_MODE` | `chat_completions` | `chat_completions` or `responses`. |
| `LIQUID_WORKBENCH_MAX_TOOL_ROUNDS` | `10` | Max tool-call rounds per chat workbench turn. |
| `LIQUID_WORKBENCH_MAX_OUTPUT_TOKENS` | unset | Optional chat workbench LLM output token limit. Unset means provider default/no explicit limit. |
| `LIQUID_SQL_METADATA` | `auto` | `auto`, `off`, or `required`. |
| `LIQUID_SQL_EXECUTION` | `readonly` | `off`, `readonly`, or `write_gated`. |
| `LIQUID_SQL_MANAGED_POOL_MAX_CONNECTIONS` | `2` | Max connections per target managed database pool. |
| `LIQUID_SQL_MANAGED_POOL_IDLE_TTL_SECONDS` | `600` | Close unused target pools after this many seconds. |
| `LIQUID_SQL_MANAGED_POOL_REAP_INTERVAL_SECONDS` | `60` | Pool reaper interval. |
| `LIQUID_SQL_MANAGED_POOL_ACQUIRE_TIMEOUT_SECONDS` | `10` | Target pool acquire timeout. |
| `LIQUID_SQLX_LOGS` | `false` | Enable pretty SQLx query logs when truthy. |
| `LIQUID_BACKUP_S3_BUCKET` | unset | Optional S3 bucket for backup uploads. Unset keeps backups local. |
| `LIQUID_BACKUP_S3_PREFIX` | `liquid/database-backups` | S3 object key prefix. |
| `LIQUID_BACKUP_S3_REGION` | `us-east-1` | S3 signing region. |
| `LIQUID_BACKUP_S3_ENDPOINT` | unset | Optional S3-compatible endpoint. |
| `LIQUID_BACKUP_S3_PATH_STYLE` | `false` | Use path-style S3 URLs. |
| `LIQUID_BACKUP_WORK_DIR` | `~/.liquid/backup` | Local backup root using `{owner_user_id}/{managed_database_id}/{backup_id}.dump`. |
| `LIQUID_BACKUP_WORKER_CONCURRENCY` | `1` | Number of database operation worker tasks. |

Boolean environment variables accept `1`, `true`, `yes`, `on`, `0`, `false`,
`no`, and `off`.

## LLM Configuration

There are two LLM configuration layers:

| Layer | Used For | Source |
| --- | --- | --- |
| Process-level | Startup SQL audit agent fallback and audit summary. | `OPENAI_*` environment variables or `[llm]` TOML values. |
| User-level | Chat workbench and per-user SQL audit creation when configured. | `/api/v1/settings/llm-provider`. |

If process-level key or model is missing, the CLI starts the mock SQL audit
agent. If user-level settings are missing, chat workbench turns are blocked until
the user configures a provider.

## Backup Worker Configuration

The backup/restore worker starts with local storage by default. It stores backup
files under `LIQUID_BACKUP_WORK_DIR` using
`{owner_user_id}/{managed_database_id}/{backup_id}.dump`.

When `LIQUID_BACKUP_S3_BUCKET` is set, the worker also uploads completed dumps
to S3-compatible storage, records the S3 bucket/key/version metadata, and keeps
the local file as a copy. S3 mode requires AWS-compatible credentials in the
process environment:

```bash
export AWS_ACCESS_KEY_ID=...
export AWS_SECRET_ACCESS_KEY=...
export AWS_SESSION_TOKEN=... # optional
```

Runtime requirements:

- `pg_dump` for backups,
- `pg_restore` for restores,
- writable `LIQUID_BACKUP_WORK_DIR`,
- network access to the managed database,
- network access to the S3-compatible endpoint when S3 upload is configured.

The current Docker runtime image installs only `ca-certificates`; add PostgreSQL
client tools to the runtime image before relying on backup/restore inside that
container.

This worker is currently backend infrastructure. The API does not expose
first-class backup or restore REST routes yet, and workbench action application
rejects backup/restore action kinds.

## Local PostgreSQL

Start a local application database:

```bash
docker run --name liquid-postgres \
  -e POSTGRES_PASSWORD=postgres \
  -e POSTGRES_DB=liquid \
  -p 5432:5432 \
  -d postgres:16
```

Then run:

```bash
cargo run -p liquid-cli -- migrate
cargo run -p liquid-cli -- server
```

The API defaults to `http://127.0.0.1:3001`.

## Frontend Runtime

Run from `liquid-ui`:

```bash
bun install
bun run dev
```

The dashboard runs on `http://localhost:3000` and reads:

```text
NEXT_PUBLIC_API_BASE_URL=http://localhost:3001
```

## Docker

Build the API image:

```bash
docker build -t liquid .
```

Run it against a host PostgreSQL instance:

```bash
docker run --rm \
  -p 3001:3001 \
  -e LIQUID_DATABASE_URL=postgres://postgres:postgres@host.docker.internal:5432/liquid \
  -e LIQUID_ENCRYPTION_KEY=replace-with-a-secret-key \
  liquid
```

The image starts `liquid server` and binds `0.0.0.0:3001` inside the container.
It does not serve `liquid-ui`.

## Operational Notes

- Keep `LIQUID_DATABASE_AUTO_MIGRATE=true` for simple local development. For
  controlled deployments, run `liquid migrate` separately and disable startup
  auto-migration.
- Rotate `LIQUID_ENCRYPTION_KEY` carefully; existing encrypted secrets depend on
  it.
- Set `LIQUID_SQL_EXECUTION=write_gated` only when the deployment is ready to
  execute approved write audits.
- Enable `LIQUID_SQLX_LOGS=true` only for local debugging because SQL text may be
  sensitive.
- Keep the frontend CORS origin aligned with the deployed dashboard URL.
