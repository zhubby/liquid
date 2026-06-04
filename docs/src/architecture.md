# Architecture

The Rust backend is split into focused crates:

- `liquid-cli` owns process startup.
- `liquid-api` owns Axum routes and HTTP server composition.
- `liquid-config` owns environment-backed configuration.
- `liquid-core` owns shared domain and transport types.
- `liquid-agent` owns SQL audit agent boundaries.
- `liquid-storage` owns Postgres and SQLx integration.

The first dashboard flow uses mock audit summary data through
`GET /api/v1/audit/summary`. Real persistence and AI audit execution should be
introduced behind the existing storage and agent boundaries.
