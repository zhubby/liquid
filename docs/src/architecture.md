# Architecture

The Rust backend is split into focused crates:

- `liquid-cli` owns Clap command parsing and process startup.
- `liquid-api` owns Axum routes and HTTP server composition.
- `liquid-config` owns environment-backed configuration.
- `liquid-core` owns shared domain and transport types.
- `liquid-agent` owns SQL audit agent boundaries.
- `liquid-llm` owns OpenAI-compatible LLM client abstractions.
- `liquid-storage` owns Postgres and SQLx integration.

The first dashboard flow uses mock audit summary data through
`GET /api/v1/audit/summary`. The CLI keeps that route stable and selects either
the mock SQL audit agent or the OpenAI-compatible tool-calling agent at startup.

The SQL audit agent uses a small tool registry and an LLM tool-call loop. The
`liquid-agent` crate owns agent behavior, tool dispatch, and SQL audit report
types. The `liquid-llm` crate owns provider protocol mapping for Chat
Completions and Responses so OpenAI wire details do not leak into agent logic.
`liquid-storage` owns the Liquid application database. It embeds SQLx
migrations and stores application users, revocable auth tokens, and managed
database connection records. Managed database connections are managed by the
frontend and stored in Liquid; they are not process-level target database
configuration.
