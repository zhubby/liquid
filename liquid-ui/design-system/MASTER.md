# Liquid UI Design System

This document is the source of truth for `liquid-ui` page design. The frontend
uses shadcn/ui primitives with the `new-york` style, `neutral` base color, and
Tailwind CSS v4 variables from `app/globals.css`.

## Design Principles

- Build the operational dashboard first; do not add a marketing landing page.
- Use shadcn/ui primitives before adding project-specific styling.
- Keep the interface dense, readable, and predictable for repeated BI work.
- Prefer neutral surfaces, subtle borders, and default shadows.
- Do not add translucent blur layers, refraction effects, or decorative
  background gradients.
- Use chart tokens and semantic shadcn tokens instead of custom color systems.

## Tokens

Use the standard shadcn CSS variables defined in `app/globals.css`:

- Page and text: `--background`, `--foreground`.
- Containers: `--card`, `--card-foreground`, `--border`.
- Controls: `--primary`, `--primary-foreground`, `--secondary`,
  `--secondary-foreground`, `--accent`, `--accent-foreground`.
- States: `--muted`, `--muted-foreground`, `--destructive`.
- Charts: `--chart-1` through `--chart-5`.
- Floating chart tooltips: `--popover`, `--popover-foreground`.

Severity colors in charts should map to existing tokens:

- Low: `--chart-2`.
- Medium: `--chart-4`.
- High: `--chart-5`.
- Critical: `--destructive`.

## Components

- `Button` should keep the native shadcn variants: `default`, `destructive`,
  `outline`, `secondary`, `ghost`, and `link`.
- `Badge` should use native shadcn variants for status display; avoid bespoke
  status classes unless a repeated state pattern needs them.
- `Card` should use the default shadcn card structure for KPI, chart, and panel
  containers.
- Recharts should read CSS variables for stroke, fill, grid, axis, and tooltip
  colors.

## Dashboard Layout

- Use `main` with `bg-background text-foreground` and responsive page padding.
- Put the app title, date range, API state, and refresh action in a sticky top
  card.
- Show KPI cards before charts.
- Keep charts in shadcn cards with stable heights such as `h-72`.
- Keep controls reachable on mobile by allowing the header actions to wrap.

## Verification

Before shipping frontend changes:

- Run `bun run lint`.
- Run `bun run build`.
- Visually check desktop and mobile viewports.
- Confirm the offline fallback still renders mock data and the refresh button
  still triggers a reload.
