# Dashboard Page

These rules apply to the SQL audit dashboard rendered by
`components/audit-dashboard.tsx`.

## Information Hierarchy

The first viewport should show:

1. Sticky app and controls card.
2. Four KPI cards.
3. Audit trend chart and risk breakdown chart.

Do not include a marketing hero, large illustration, or explanatory feature
section.

## Header Controls

Use one sticky shadcn card at the top.

Required contents:

- left: `ShieldCheck` icon, `Liquid SQL Audit`, compact subtitle or environment
  label,
- right: date range display, API status badge, refresh button.

Behavior:

- Desktop: one row with title on the left and controls on the right.
- Mobile: allow controls to wrap below the title.
- Do not use custom material, blur, or refraction effects.

## KPI Cards

Card order:

1. Audited queries.
2. Flagged queries.
3. Audit score.
4. Average latency.

Rules:

- Use default shadcn `Card`, `CardHeader`, `CardTitle`, and `CardContent`.
- Keep value baseline alignment stable.
- Keep all icon wells the same size.
- Use existing shadcn/chart tokens for visual emphasis.

## Chart Panels

`Audit Volume`:

- Use a line chart.
- Audited line: `--chart-2`.
- Flagged line: `--destructive`.
- Tooltip uses `--popover`, `--popover-foreground`, and `--border`.

`Risk Breakdown`:

- Use a vertical bar chart for the current data shape.
- Preserve severity meaning with existing tokens:
  - low: `--chart-2`,
  - medium: `--chart-4`,
  - high: `--chart-5`,
  - critical: `--destructive`.
- Keep labels visible at 375px; abbreviate labels only if tooltip gives the full
  label.

## State Handling

API state appears in the control card:

- Live API: secondary badge.
- Loading: outline badge with refresh icon motion.
- Mock data/offline: destructive badge.

Avoid placing offline state as a dominant banner unless the data is unusable.

## Implementation Notes

- Keep `NEXT_PUBLIC_API_BASE_URL` behavior unchanged.
- Keep Recharts containers at `h-72` or `h-80`.
- Use CSS variables for chart colors instead of hardcoded hex values.
- Do not introduce extra dependencies for visual effects.
