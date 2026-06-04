# Development

## Backend

```bash
cargo run -p liquid-cli
```

The API binds to `LIQUID_API_ADDR`, defaulting to `127.0.0.1:3001`.

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
