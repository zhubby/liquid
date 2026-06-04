# Liquid UI Apple Liquid Glass Design System

This document is the source of truth for `liquid-ui` page design. It adapts
Apple's Liquid Glass direction to a Next.js, Tailwind, shadcn/ui, Recharts BI
dashboard without pretending the web has the same native material APIs as Apple
platforms.

References:

- Apple new design gallery: https://developer.apple.com/cn/design/new-design-gallery/
- Apple Human Interface Guidelines: https://developer.apple.com/design/human-interface-guidelines/
- React implementation dependency: https://github.com/rdev/liquid-glass-react

## Design Objective

Liquid UI should feel like a precise operational BI tool with a calm Liquid
Glass control layer. The page must make SQL audit telemetry easy to scan first;
glass effects support hierarchy and focus, but never compete with metrics,
tables, charts, or alerts.

The target impression is:

- clear data surfaces with crisp type and stable chart geometry,
- floating controls that visually respond to content behind them,
- restrained depth from blur, translucency, border light, and shadow,
- no decorative glass overload,
- no marketing landing-page composition.

## Product Context

- Product type: BI dashboard / SQL audit console.
- Audience: engineers, security reviewers, data platform operators.
- Stack: Next.js App Router, React, Tailwind CSS v4, shadcn/ui primitives,
  Recharts, lucide-react.
- Primary page: dashboard-first operational surface.
- Existing entry point: `app/page.tsx` renders `components/audit-dashboard.tsx`.

## Non-Negotiable Principles

1. Content is the base layer. Metrics, charts, tables, and logs stay opaque
   enough for sustained reading.
2. Glass is the control layer. Use Liquid Glass for nav bars, toolbars, filters,
   command surfaces, segmented controls, popovers, and transient status panels.
3. One glass emphasis per viewport. If the top bar is glass, cards should be
   mostly solid. If a modal or popover is active, surrounding glass recedes.
4. Depth must encode function. Raised glass means interactive or transient;
   flat solid surfaces mean persistent information.
5. Contrast wins over material fidelity. Any text over glass must pass WCAG AA.
6. Motion is subtle and optional. Respect `prefers-reduced-motion`; never use
   continuous decorative animation.
7. Do not imitate iOS chrome literally. Translate the material behavior to web
   controls that fit a dense dashboard.

## Page Model

Use three layers:

### 1. Environment Layer

The environment layer is the page background. It provides enough color and
shape for glass to refract against, but remains quiet behind dense data.

Use:

- soft off-white base in light mode,
- near-black graphite base in dark mode,
- two large, low-contrast radial color fields anchored outside the viewport,
- no discrete orbs, bokeh dots, decorative blobs, or noisy gradients.

### 2. Content Layer

The content layer contains KPI cards, charts, risk lists, tables, SQL samples,
and empty states.

Use:

- solid or near-solid surfaces,
- 8px radius maximum for repeated dashboard cards,
- visible borders in both light and dark mode,
- compact spacing and predictable grid tracks,
- chart canvases with stable height and no layout shift.

### 3. Liquid Control Layer

The control layer contains navigation, filters, refresh controls, date range
selection, search, popovers, and contextual commands.

Use:

- translucent fill,
- `backdrop-filter: blur(...) saturate(...)`,
- thin inner and outer border highlights,
- low, soft shadow,
- raised z-index with explicit stacking contexts,
- rounded capsule shapes only for actual bars, segmented controls, pills, and
  icon buttons.

## Core Tokens

Define these as CSS custom properties in `app/globals.css`. Tailwind utilities
should consume the variables rather than hardcoded colors inside components.

### Light Mode

```css
:root {
  --background: #f6f8fb;
  --foreground: #101418;

  --surface-1: #ffffff;
  --surface-2: #f9fafc;
  --surface-3: #eef2f7;

  --glass-fill: rgba(255, 255, 255, 0.66);
  --glass-fill-strong: rgba(255, 255, 255, 0.78);
  --glass-border: rgba(255, 255, 255, 0.72);
  --glass-edge: rgba(15, 23, 42, 0.12);
  --glass-shadow: 0 18px 54px rgba(15, 23, 42, 0.14);
  --glass-inner-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.82);

  --primary: #0b5cff;
  --primary-foreground: #ffffff;
  --accent-cyan: #00a6c8;
  --accent-mint: #2fb67c;
  --warning: #b7791f;
  --danger: #c24130;
  --critical: #9f1239;

  --muted: #e8edf5;
  --muted-foreground: #4b5565;
  --border: #d9e0ea;
  --ring: #0b5cff;
}
```

### Dark Mode

```css
@media (prefers-color-scheme: dark) {
  :root {
    --background: #080a0d;
    --foreground: #f5f7fb;

    --surface-1: #11151b;
    --surface-2: #161b22;
    --surface-3: #202733;

    --glass-fill: rgba(22, 27, 34, 0.62);
    --glass-fill-strong: rgba(22, 27, 34, 0.78);
    --glass-border: rgba(255, 255, 255, 0.18);
    --glass-edge: rgba(255, 255, 255, 0.08);
    --glass-shadow: 0 20px 64px rgba(0, 0, 0, 0.42);
    --glass-inner-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.16);

    --primary: #5c9dff;
    --primary-foreground: #06111f;
    --accent-cyan: #37c6e6;
    --accent-mint: #55d89d;
    --warning: #f2b84b;
    --danger: #ff775f;
    --critical: #ff5d8f;

    --muted: #1d2430;
    --muted-foreground: #a6b0c2;
    --border: #2a3342;
    --ring: #8bb8ff;
  }
}
```

## Material Recipes

### Native-Style Liquid Glass

Use `liquid-glass-react` for the primary glass control surface when the effect
needs stronger refraction than CSS can provide. In `liquid-ui`, this applies to
the floating app/control bar and can later be used for high-value popovers or
command surfaces.

Rules:

- Do not server-render the library directly. It reads browser globals, so wrap
  usage with a mounted client-state fallback.
- Keep CSS `.liquid-bar` as the SSR and unsupported-browser fallback.
- Do not wrap persistent KPI, chart, table, or log cards in `LiquidGlass`.
- Use conservative settings for dashboards:
  `displacementScale` 36-52, `blurAmount` 0.02-0.05, `saturation` 140-165,
  `aberrationIntensity` 1.2-2.0.
- Avoid `mode="shader"` until it is explicitly tested; prefer `prominent` for
  top bars and `standard` for smaller controls.
- Safari and Firefox only partially show the displacement effect; the UI must
  still look intentional with the CSS fallback styling.

### Liquid Bar

Use for top navigation, section toolbars, and sticky filter bars.

```css
.liquid-bar {
  background:
    linear-gradient(135deg, rgba(255, 255, 255, 0.44), transparent 38%),
    var(--glass-fill);
  border: 1px solid var(--glass-border);
  box-shadow: var(--glass-shadow), var(--glass-inner-shadow);
  backdrop-filter: blur(28px) saturate(1.55);
  -webkit-backdrop-filter: blur(28px) saturate(1.55);
}
```

Rules:

- Use `rounded-full` only when the bar is detached from page edges.
- Keep top bars inset from the viewport: `top-4 left-4 right-4`.
- Add content padding so fixed bars never cover page content.
- Use icon buttons for repeated tools; add text only to primary commands.

### Liquid Button

Use for controls that sit on the glass layer.

```css
.liquid-button {
  background: var(--glass-fill-strong);
  border: 1px solid var(--glass-border);
  box-shadow: var(--glass-inner-shadow);
  backdrop-filter: blur(18px) saturate(1.35);
  -webkit-backdrop-filter: blur(18px) saturate(1.35);
}
```

Rules:

- Minimum size: 36px desktop, 40px touch targets on mobile.
- Use lucide icons for refresh, filters, date, search, export, settings.
- Hover can change fill, border, and shadow; it must not scale or shift layout.
- Focus uses a visible ring with at least 2px outline.

### Solid Data Card

Use for KPI cards, chart panels, and persistent dashboard modules.

```css
.data-card {
  background: color-mix(in srgb, var(--surface-1) 92%, transparent);
  border: 1px solid var(--border);
  border-radius: 8px;
  box-shadow: 0 1px 2px rgba(15, 23, 42, 0.04);
}
```

Rules:

- Do not make every dashboard card glass.
- KPI cards can use a subtle top highlight, but values must sit on a stable
  solid field.
- Chart cards use fixed heights: 280px, 320px, or 360px depending on density.
- Avoid nested cards; use separators, grids, or section headers instead.

### Liquid Popover

Use shadcn/Radix Popover for floating filters, export menus, and command
surfaces.

Rules:

- Use `Popover`, `PopoverTrigger`, `PopoverContent`; avoid custom absolute
  dropdowns.
- Set `align` and `side` explicitly.
- Use stronger fill than bars: `--glass-fill-strong`.
- Popovers own a high z-index token and do not depend on `z-9999`.
- Include keyboard focus order and Escape dismissal.

## Layout Specification

### Desktop

- Max content width: `max-w-7xl`.
- Page padding: `px-6 lg:px-8`.
- Top bar: fixed or sticky, inset `16px`, height `56px`.
- Header zone: compact, no hero-scale type.
- KPI grid: 4 columns at xl, 2 columns at md, 1 column mobile.
- Main chart grid: `xl:grid-cols-[1.6fr_1fr]`.
- Repeated panel gap: `16px`.

### Tablet

- Top controls may wrap into two rows inside the liquid bar.
- Keep icon buttons fixed-size so wrapping does not resize controls.
- Chart labels may abbreviate but must retain tooltip/detail access.

### Mobile

- No horizontal scroll at 375px.
- Top bar can become a bottom-safe floating toolbar for high-frequency actions.
- KPI cards stack vertically.
- Chart panels keep at least 260px height.
- Table-heavy views must provide filter chips and row summaries before full
  detail drawers.

## Typography

Use system fonts unless the project intentionally adds `next/font`. Do not load
remote Google Fonts for the dashboard by default.

Recommended stack:

```css
font-family:
  ui-sans-serif,
  -apple-system,
  BlinkMacSystemFont,
  "SF Pro Display",
  "SF Pro Text",
  "Segoe UI",
  sans-serif;
```

Type scale:

- Page title: 24px / 32px, 600.
- Section title: 15px / 22px, 600.
- KPI value: 30px / 36px, 650.
- Body: 14px / 22px, 400.
- Metadata: 12px / 18px, 500.
- Monospace SQL: 13px / 20px, `ui-monospace`, `SFMono-Regular`,
  `Menlo`, `Monaco`, monospace.

Rules:

- Letter spacing is `0`.
- Do not scale type with viewport width.
- Keep headings compact inside cards and tool surfaces.
- Muted text must remain readable: use `--muted-foreground`, not low-opacity
  foreground on glass.

## Color Semantics

The dashboard cannot be one-note blue, gray, or purple. Use blue for primary
actions and selected states, mint for healthy/safe status, amber for warnings,
red for high risk, and rose for critical events.

Risk colors:

- Low: `--accent-mint`
- Medium: `--warning`
- High: `--danger`
- Critical: `--critical`

Rules:

- Do not encode risk by color only; include label, icon, or text.
- Use stronger fills for tiny badges than for large panels.
- Chart colors must stay distinct in dark and light mode.

## Component Rules

### App Shell

The app shell should present the product identity, environment state, time
range, refresh/export controls, and settings as a single floating liquid bar.

Required controls:

- product mark/name,
- API status badge,
- date range selector,
- filter trigger,
- refresh icon button,
- export icon button when data export exists.

Do not place the entire page header in a card.

### KPI Cards

KPI cards are solid data cards with small icon wells.

Rules:

- Icon wells can be softly tinted, not fully glass.
- Values align consistently across cards.
- Detail text is optional but must reserve stable space when used in a row.
- Avoid oversized icons.

### Charts

Use Recharts for line, bar, area, and composed charts.

Rules:

- Default line width: 2px or 2.5px; avoid 3px+ unless a single trend is shown.
- Grid lines are low contrast but visible.
- Tooltip uses liquid popover styling only if contrast remains strong.
- Provide table or textual alternatives for complex charts such as treemap,
  Sankey, network, or geographic views.
- Preserve stable dimensions through loading, error, and empty states.

### Tables And Logs

Tables and SQL logs should be mostly solid, not glass.

Rules:

- Sticky table headers may use a light glass material.
- Row hover uses background color, not scale.
- SQL text uses monospace and wraps intentionally.
- Never log or display raw sensitive SQL unless the feature explicitly handles
  redaction.

### Badges

Badges are for status and severity, not decoration.

Rules:

- Use compact radius and strong contrast.
- Live/offline/loading states use icon plus label where space allows.
- Critical state must not rely on translucency alone.

### Empty And Loading States

Loading states should preserve layout.

Rules:

- Use skeleton blocks on data cards.
- Use a spinner only for isolated icon buttons or inline refresh.
- Do not animate decorative glass backgrounds.

## Motion

Default transition:

```css
transition-property: background-color, border-color, color, box-shadow, opacity;
transition-duration: 180ms;
transition-timing-function: cubic-bezier(0.16, 1, 0.3, 1);
```

Allowed:

- hover color shifts,
- focus rings,
- popover fade/slide under 160ms,
- toolbar shadow change while scrolling.

Avoid:

- continuous shimmer except skeleton loading,
- parallax,
- scroll-jacking,
- scale hover on dashboard cards,
- more than 1-2 animated elements per view.

Reduced motion:

```css
@media (prefers-reduced-motion: reduce) {
  *,
  *::before,
  *::after {
    animation-duration: 0.01ms !important;
    animation-iteration-count: 1 !important;
    scroll-behavior: auto !important;
    transition-duration: 0.01ms !important;
  }
}
```

## Accessibility

- Normal text contrast: at least 4.5:1.
- Large text and icons: at least 3:1.
- Focus state: visible on every button, link, trigger, and chart control.
- Touch target: 40px minimum on mobile.
- Keyboard: all popovers, menus, segmented controls, and filters must be usable
  without a pointer.
- Motion: respect `prefers-reduced-motion`.
- Backdrop support: if `backdrop-filter` is unavailable, glass surfaces must
  fall back to an opaque `--surface-1` or `--surface-2`.

Fallback:

```css
@supports not ((backdrop-filter: blur(1px)) or (-webkit-backdrop-filter: blur(1px))) {
  .liquid-bar,
  .liquid-button,
  .liquid-popover {
    background: var(--surface-1);
  }
}
```

## Implementation Mapping

### CSS

Add reusable classes in `app/globals.css`:

- `.liquid-environment`
- `.liquid-bar`
- `.liquid-button`
- `.liquid-popover`
- `.data-card`
- `.chart-panel`
- `.severity-low`
- `.severity-medium`
- `.severity-high`
- `.severity-critical`

### shadcn/ui

Update base primitives conservatively:

- `Button`: add `cursor-pointer`, stable focus ring, optional `liquid` variant.
- `Card`: keep default card solid; add `data-card` class at composition sites.
- Add Popover only when needed; keep it Radix-backed.

### Next.js

- Keep static shell pieces as Server Components where possible.
- Keep data-fetching and chart interactions inside client components.
- Apply fonts once in `app/layout.tsx`, not per page.

## Anti-Patterns

Do not:

- make every card translucent,
- put cards inside cards,
- use a marketing hero as the dashboard first screen,
- use emoji as UI icons,
- use SVG blobs, bokeh, or decorative orbs,
- hardcode API hosts,
- hide content behind fixed bars,
- use `z-index: 9999` as a layout strategy,
- depend on color alone for risk,
- use body text lighter than `--muted-foreground`,
- animate the background continuously,
- use purple-blue gradients as the dominant visual system.

## Page Acceptance Checklist

Before shipping a Liquid UI page:

- [ ] First screen is the usable dashboard, not a landing page.
- [ ] Glass is limited to navigation, controls, popovers, or transient panels.
- [ ] Persistent metric/chart/table surfaces are readable and mostly solid.
- [ ] Top or bottom floating controls do not cover content at 375px, 768px,
  1024px, or 1440px.
- [ ] Every clickable item has pointer affordance and visible focus state.
- [ ] Hover states do not change layout dimensions.
- [ ] Light and dark mode glass borders remain visible.
- [ ] Chart containers keep stable height during loading and errors.
- [ ] Risk states include text or icons, not color only.
- [ ] `prefers-reduced-motion` is respected.
- [ ] `backdrop-filter` fallback is defined.
- [ ] `bun run lint` and `bun run build` pass for frontend changes.
