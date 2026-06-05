# Development

## Backend

```bash
cargo run -p liquid-cli
```

The API binds to `LIQUID_API_ADDR`, defaulting to `127.0.0.1:3001`.
You can also provide a TOML config file:

```bash
cargo run -p liquid-cli -- --config liquid.toml
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
```

Environment variables override config file values. The Liquid application
database stores users, auth tokens, and user-managed audited database connection
records. Audited database passwords are encrypted with `LIQUID_ENCRYPTION_KEY`
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

PostgreSQL metadata tools use `DATABASE_URL`. SQL metadata collection is
controlled by `LIQUID_SQL_METADATA=auto|off|required`, defaulting to `auto`.
Agent SQL execution tools are controlled separately by
`LIQUID_SQL_EXECUTION=off|readonly|write_gated`, defaulting to `readonly`.
The gated write tool is only registered when `LIQUID_SQL_EXECUTION=write_gated`.

## Frontend

```bash
cd liquid-ui
bun install
bun run dev
```

The dashboard reads `NEXT_PUBLIC_API_BASE_URL`, defaulting to
`http://localhost:3001`.

## Docs

```bash
mdbook serve docs
```
