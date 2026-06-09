# Development

Liquid is a Rust workspace plus a Bun-managed Next.js app. Run Rust commands
from the repository root and frontend commands from `liquid-ui`.

## Requirements

- Rust stable with Rust 2024 support.
- Bun for frontend development.
- PostgreSQL for the Liquid application database.
- Docker, optional for local PostgreSQL and API image builds.
- mdBook for documentation preview.
- PostgreSQL client tools, optional for backup/restore worker development.

## Backend Commands

```bash
cargo check --workspace
cargo test --workspace
cargo fmt --all
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

Start the API:

```bash
cargo run -p liquid-cli -- server
```

Run migrations only:

```bash
cargo run -p liquid-cli -- migrate
```

Print the binary version:

```bash
cargo run -p liquid-cli -- version
```

## Frontend Commands

```bash
cd liquid-ui
bun install
bun run dev
bun run lint
bun run build
```

The frontend reads `NEXT_PUBLIC_API_BASE_URL`, defaulting to
`http://localhost:3001`.

## Documentation Commands

Preview docs:

```bash
mdbook serve docs
```

Verify docs:

```bash
mdbook build docs
```

Every page under `docs/src` should be linked from `docs/src/SUMMARY.md`.

## Type Contract Workflow

Frontend API contracts are generated from Rust DTOs in `liquid-core`.

When changing a type used by the frontend:

1. Change the Rust DTO in `liquid-core`.
2. Run:

   ```bash
   cargo test -p liquid-core
   ```

3. Review generated files under `liquid-ui/lib/generated/api-types`.
4. Update frontend imports or rendering code as needed.
5. Commit the Rust and generated TypeScript changes together.

Do not edit generated API type files by hand.

## Backend Development Patterns

- Keep route handlers in `liquid-api` thin. Move persistent behavior to
  `liquid-storage`, SQL analysis to `liquid-sql`, and agent/tool behavior to
  `liquid-agent`.
- Keep shared request and response DTOs in `liquid-core`.
- Use `LiquidStore` in API code instead of reaching into concrete SQLx queries.
- Use `ManagedDatabaseConnectionLoader` when a component needs target database
  connection specs.
- Keep LLM provider wire details inside `liquid-llm`.
- Do not use `.unwrap()` or `.expect()` in production paths.
- Prefer typed enums over string matching except at serialization, database, or
  display boundaries.

## Frontend Development Patterns

- Use `apiRequest`, `apiRequestWithMeta`, and `apiStream` from
  `liquid-ui/lib/api.ts`.
- Import generated DTO types through `@/lib/api`.
- Keep dashboard-specific composition in feature components.
- Use existing `components/ui` primitives, lucide-react icons, and Recharts.
- Keep the first authenticated screen operational: database selection followed by
  the split workspace, not a marketing page.
- For streamed chat changes, keep event handling compatible with
  `ChatStreamEvent`.

## Test Focus

Use targeted tests for the layer being changed:

| Change Area | Useful Verification |
| --- | --- |
| CLI/config startup | `cargo test -p liquid-cli`, `cargo test -p liquid-config`, manual `cargo run -p liquid-cli -- version`. |
| API route behavior | `cargo test -p liquid-api`. |
| SQL parsing or risk rules | `cargo test -p liquid-sql`; performance baseline with `cargo bench -p liquid-sql --bench postgres_static_analysis`. |
| Storage queries or migrations | `cargo test -p liquid-storage` with a test PostgreSQL database. |
| Contract DTOs | `cargo test -p liquid-core`. |
| Frontend behavior | `bun run lint` and `bun run build` from `liquid-ui`. |
| Docs | `mdbook build docs`. |

The storage integration tests require PostgreSQL access. Use the default local
database URL unless the test harness is configured otherwise:

```text
postgres://postgres:postgres@localhost:5432/liquid
```

## Local End-to-End Run

Start local PostgreSQL:

```bash
docker run --name liquid-postgres \
  -e POSTGRES_PASSWORD=postgres \
  -e POSTGRES_DB=liquid \
  -p 5432:5432 \
  -d postgres:16
```

Run backend:

```bash
cargo run -p liquid-cli -- migrate
cargo run -p liquid-cli -- server
```

Run frontend in another terminal:

```bash
cd liquid-ui
bun install
bun run dev
```

Open `http://localhost:3000`, register a user, create a managed PostgreSQL
database record, and enter the workspace.

## Common Change Recipes

Add an API response field:

1. Add the field to the Rust type in `liquid-core`.
2. Populate it in storage or API mapping code.
3. Run `cargo test -p liquid-core`.
4. Use the generated TypeScript field in frontend code.
5. Run relevant backend tests and `bun run lint`.

Add a route:

1. Add route state and handler in `liquid-api`.
2. Put durable behavior behind `LiquidStore` or another trait boundary.
3. Add or update DTOs in `liquid-core`.
4. Add route tests in `liquid-api/tests`.
5. Update [API Reference](./api-reference.md).

Add a storage-backed feature:

1. Add a SQLx migration in `liquid-storage/migrations`.
2. Add concrete storage functions and trait methods.
3. Add storage tests.
4. Expose typed DTOs from `liquid-core`.
5. Wire API routes and frontend calls.
6. Update [Data Model](./data-model.md).

Add or change SQL safety behavior:

1. Update `liquid-sql` parser or rule code.
2. Add focused tests in `liquid-sql`.
3. Update audit lifecycle behavior in `liquid-api` only if statuses change.
4. Update [SQL Safety Model](./sql-safety-model.md).
