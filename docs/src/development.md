# Development

## Backend

```bash
cargo run -p liquid-cli -- server
```

The API binds to `LIQUID_API_ADDR`, defaulting to `127.0.0.1:3001`.
When `--config` is omitted, Liquid reads `~/.liquid/config.toml`. If the file
does not exist, the CLI creates `~/.liquid/` and writes a default config file
before loading it. You can also provide a TOML config file explicitly:

```bash
cargo run -p liquid-cli -- server --config liquid.toml
```

Run application database migrations without starting the API:

```bash
cargo run -p liquid-cli -- migrate
```

`migrate` uses the same default config lookup and creation behavior as
`server`.

Print the binary version:

```bash
cargo run -p liquid-cli -- version
```

Example:

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
encryption_key = "replace-with-a-secret-key"

[llm]
base_url = "https://api.openai.com"
api_mode = "chat_completions"

[sql]
metadata = "auto"
execution = "readonly"
managed_pool_max_connections = 2
managed_pool_idle_ttl_seconds = 600
managed_pool_reap_interval_seconds = 60
managed_pool_acquire_timeout_seconds = 10
```

Environment variables override config file values. The Liquid application
database stores users, auth tokens, and managed database connection records.
Managed database passwords are encrypted with `LIQUID_ENCRYPTION_KEY`
or `[security].encryption_key`.

The backend uses the mock SQL audit agent unless both `OPENAI_API_KEY` and
`OPENAI_MODEL` are set. OpenAI-compatible LLM settings are:

```bash
export OPENAI_API_KEY=...
export OPENAI_MODEL=gpt-5.4
export OPENAI_BASE_URL=https://api.openai.com
export OPENAI_API_MODE=chat_completions
```

`OPENAI_BASE_URL` defaults to `https://api.openai.com` and can be provided with
or without a trailing `/v1`. `OPENAI_API_MODE` defaults to `chat_completions` and
also supports `responses`.

Managed database SQL audit tools use the saved managed database records, not the
process-level `DATABASE_URL`. SQL metadata collection is controlled by
`LIQUID_SQL_METADATA=auto|off|required`, defaulting to `auto`.
`LIQUID_SQL_EXECUTION=off` disables managed audit execution tools, `readonly`
enables read-only execution, and `write_gated` exposes gated write execution for
audited, user-approved statements. Each managed database instance gets a lazy
SQLx pool that is closed after
`LIQUID_SQL_MANAGED_POOL_IDLE_TTL_SECONDS` seconds without use.

## Frontend

```bash
cd liquid-ui
bun install
bun run dev
```

The dashboard reads `NEXT_PUBLIC_API_BASE_URL`, defaulting to
`http://localhost:3001`.

## API Type Contracts

Frontend API contracts are generated from `liquid-core` with ts-rs. Do not edit
files under `liquid-ui/lib/generated/api-types` by hand.

After changing Rust DTOs used by the frontend, run:

```bash
cargo test -p liquid-core
```

Commit the regenerated TypeScript files with the Rust contract change.

## Docs

```bash
mdbook serve docs
```
