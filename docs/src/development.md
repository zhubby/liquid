# Development

## Backend

```bash
cargo run -p liquid-cli
```

The API binds to `LIQUID_API_ADDR`, defaulting to `127.0.0.1:3001`.

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
