# Dashboard Page Liquid Glass Overrides

These rules override `../MASTER.md` for the main SQL audit dashboard rendered by
`components/audit-dashboard.tsx`.

## Information Hierarchy

The dashboard first viewport must show:

1. Floating liquid app/control bar.
2. Four KPI cards.
3. Audit trend chart and risk breakdown chart.

The page should not include a marketing hero, large illustration, or explanatory
feature section.

## Floating Control Bar

Use one detached liquid bar at the top.

Required contents:

- left: `ShieldCheck` icon, `Liquid SQL Audit`, compact subtitle or environment
  label,
- center or wrap row: time range segmented control when implemented,
- right: API status badge, filter button, refresh button.

Behavior:

- Desktop: sticky top, inset from page edges, rounded capsule or rounded 16px
  depending on wrapped controls.
- Mobile: allow two rows inside the bar; keep actions reachable and fixed-size.
- Do not place the bar inside a `Card`.

## KPI Row

KPI cards should remain solid data cards. Use glass only for tiny icon wells if
needed.

Card order:

1. Audited queries.
2. Flagged queries.
3. Audit score.
4. Average latency.

Rules:

- Keep value baseline alignment stable.
- Keep all icon wells the same size.
- Use semantic severity colors for flagged and high-risk values.
- Do not animate KPI cards on load unless skeletons are used.

## Chart Panels

`Audit Volume`:

- Use a line or composed chart.
- Audited line: primary blue.
- Flagged line: danger red.
- Optional safe region or threshold band may use mint at low opacity.
- Tooltip can use liquid popover styling with solid text contrast.

`Risk Breakdown`:

- Use vertical bar chart for current data shape.
- Preserve severity colors:
  - low: mint,
  - medium: amber,
  - high: red,
  - critical: rose.
- Keep labels visible at 375px; abbreviate labels only if tooltip gives the full
  label.

## State Handling

API state appears in the control bar:

- Live API: blue or mint badge.
- Loading: neutral badge with refresh icon motion only if reduced motion allows.
- Mock data/offline: amber badge.

Avoid placing offline state as a dominant banner unless the data is unusable.

## Implementation Notes

When implementing this page:

- Use `main` with `.liquid-environment`.
- Add top padding equal to sticky bar height plus 24px.
- Apply `.data-card` to KPI and chart containers.
- Keep Recharts containers at `h-72` or `h-80`.
- Use CSS variables for chart colors instead of hardcoded earth-tone hex values.
- Keep `NEXT_PUBLIC_API_BASE_URL` behavior unchanged.

