# Repository Guidelines

## Project Structure & Module Organization

This repository is a Rust workspace with a Next.js BI dashboard and mdBook docs.
Keep code in the crate or package that owns the concern:

- `liquid-cli`: process startup and CLI binary (`liquid`).
- `liquid-api`: Axum routes, API state, HTTP server composition.
- `liquid-config`: environment-backed configuration.
- `liquid-core`: shared domain and transport types.
- `liquid-agent`: SQL AI audit agent traits and implementations.
- `liquid-llm`: OpenAI-compatible LLM client abstractions.
- `liquid-storage`: Postgres and SQLx storage integration.
- `liquid-ui/`: Bun-managed Next.js + TypeScript + Tailwind + shadcn/ui + Recharts frontend.
- `docs/`: mdBook sources under `docs/src`.

Avoid leaking CLI-specific logic into API/core/storage crates. Put shared request
and response shapes in `liquid-core` when both backend and frontend-facing APIs
depend on the shape.

## Build, Test, and Development Commands

Run Rust commands from the repository root:

- `cargo check --workspace`: fast compile verification.
- `cargo test --workspace`: run unit and integration tests.
- `cargo fmt --all`: apply Rust formatting.
- `cargo fmt --all --check`: verify Rust formatting.
- `cargo clippy --workspace --all-targets -- -D warnings`: strict Rust lint pass.
- `cargo run -p liquid-cli`: start the API server.

The API binds to `LIQUID_API_ADDR`, defaulting to `127.0.0.1:3001`.
`DATABASE_URL` defaults to `postgres://postgres:postgres@localhost:5432/liquid`.
The current mock audit summary endpoint does not require a live database.

Run frontend commands from `liquid-ui/`:

- `bun install`: install frontend dependencies.
- `bun run dev`: start Next.js on port `3000`.
- `bun run lint`: run ESLint.
- `bun run build`: create a production build.

The frontend reads `NEXT_PUBLIC_API_BASE_URL`, defaulting to
`http://localhost:3001`.

For docs:

- `mdbook build docs`: verify documentation.
- `mdbook serve docs`: preview locally.

## Rust Style and Idioms

- Target Rust 2024 for new crates, modules, examples, and docs.
- Keep dependencies centralized in root `[workspace.dependencies]`; member crates
  should use `{ workspace = true }` for external workspace dependencies.
- Use path dependencies for internal crates, following the existing
  `liquid-* = { path = "../liquid-*" }` pattern.
- Use traits for behavior boundaries such as AI agents and storage interfaces.
- Prefer concrete `struct`/`enum` types over `serde_json::Value` when the shape is known.
- Match on typed values rather than strings; convert to strings only at display or
  serialization boundaries.
- Use `anyhow::Result` for application-level errors. Add crate-local error enums
  only when callers need to handle specific failure cases.
- Do not use `.unwrap()` or `.expect()` in production paths. Propagate errors with
  `?`, use `ok_or_else`, or provide explicit fallback behavior.
- Prefer guard clauses and `let-else` over deeply nested branching.
- Keep public APIs small and name modules by ownership, not implementation detail.

## Backend API and Storage Guidelines

- Keep route handlers thin. Business logic should live behind `liquid-agent`,
  `liquid-storage`, or dedicated service boundaries as the project grows.
- Keep response DTOs stable and serializable from `liquid-core`.
- Do not require a database connection for health checks.
- Introduce SQLx migrations alongside storage changes that require schema state.
- Prefer Postgres-specific SQL only in `liquid-storage`; keep higher-level crates
  database-agnostic where practical.

## Frontend Guidelines

- Build the actual BI dashboard experience as the first screen; do not add a
  marketing landing page unless explicitly requested.
- Use shadcn/ui-style primitives in `components/ui` and keep dashboard-specific
  composition in feature components.
- Use Recharts for BI charts and lucide-react for icons.
- Keep UI dense, readable, and operational: prioritize scan-friendly metrics,
  predictable controls, and restrained visual styling.
- Keep API URLs behind `NEXT_PUBLIC_API_BASE_URL`; do not hardcode production hosts.
- Bun is the package manager for this project. Do not commit npm/yarn/pnpm lockfiles.
  If another package manager is used only for local verification, keep it out of
  committed project metadata.

## Testing Guidelines

- Place Rust unit tests next to implementation in `mod tests`.
- Put integration tests under crate-local `tests/` directories when behavior spans
  modules or public APIs.
- Name tests by behavior, for example `healthz_returns_ok`.
- Add route tests for API behavior and regression tests for bug fixes.
- For frontend changes, run `bun run lint` and `bun run build`.
- For documentation structure changes, run `mdbook build docs`.

## Documentation Guidelines

When adding or updating docs under `docs/src`:

- Link every new page from `docs/src/SUMMARY.md`.
- Use clear heading hierarchy and stable section names.
- Use fenced code blocks with language tags for commands and config snippets.
- Keep examples aligned with the current binary name (`liquid`) and package paths.
- Prefer relative links for internal pages and full URLs for external references.

## Security & Configuration

- Never commit API keys, database credentials, model provider tokens, or local
  `.env` files.
- Prefer environment variables for secrets and deployment-specific configuration.
- Redact credentials from docs, examples, test output, and issue/PR descriptions.
- Treat SQL audit input as untrusted. Avoid logging raw SQL text unless a caller
  explicitly requests it and sensitive data handling is clear.

## Commit & Pull Request Guidelines

Use Conventional Commits:

```text
<type>(<scope>): <subject>
```

Common types: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`,
`chore`, `ci`, `build`, `revert`.

Keep the subject imperative, lowercase, and without a trailing period. Each commit
should be one logical change.

PRs should include:

- purpose and impacted crates/packages,
- test evidence with commands run and results,
- config or doc updates when behavior changes,
- screenshots or sample output for user-facing UI/API changes.
