"use client";

import { type ComponentProps } from "react";
import {
  Area,
  AreaChart,
  Bar,
  BarChart,
  CartesianGrid,
  Cell,
  ComposedChart,
  Funnel,
  FunnelChart,
  LabelList,
  Line,
  LineChart,
  Pie,
  PieChart,
  PolarAngleAxis,
  PolarGrid,
  PolarRadiusAxis,
  Radar,
  RadarChart,
  RadialBar,
  RadialBarChart,
  Scatter,
  ScatterChart,
  SunburstChart,
  Treemap,
  XAxis,
  YAxis,
  ZAxis,
} from "recharts";

import {
  ChartContainer,
  ChartLegend,
  ChartLegendContent,
  type ChartConfig,
  ChartTooltip,
  ChartTooltipContent,
} from "@/components/ui/chart";
import {
  type DatapanelChartConfig,
  type DatapanelChartSeries,
} from "@/lib/api";

type RowValue = Record<string, unknown>;
type ChartRow = Record<string, string | number | null>;
type ChartVariant = "card" | "preview";
type ScatterAxisMode = "number" | "time" | "category";
type ScatterXAxis = {
  mode: ScatterAxisMode;
  categories?: string[];
};
type ScatterXValue =
  | { kind: "number"; value: number }
  | { kind: "time"; label: string; value: number }
  | { kind: "category"; label: string };
type ScatterPoint = {
  x: ScatterXValue;
  y: number;
  z: number | null;
};

type DatapanelChartRendererProps = {
  chart: DatapanelChartConfig;
  rows: unknown[];
  variant?: ChartVariant;
  emptyLabel: string;
};

type HierarchyNode = {
  name: string;
  value?: number;
  fill?: string;
  children?: HierarchyNode[];
};

const chartTokens = [
  "var(--chart-1)",
  "var(--chart-2)",
  "var(--chart-3)",
  "var(--chart-4)",
  "var(--chart-5)",
  "var(--chart-6)",
  "var(--chart-7)",
  "var(--chart-8)",
  "var(--chart-9)",
  "var(--chart-10)",
];

const chartConfig = chartTokens.reduce<ChartConfig>((config, color, index) => {
  config[`series-${index}`] = { color };
  return config;
}, {});

const chartBase = {
  grid: "var(--border)",
  text: "var(--muted-foreground)",
};

export function DatapanelChartRenderer({
  chart,
  rows,
  variant = "card",
  emptyLabel,
}: DatapanelChartRendererProps) {
  const compact = variant === "preview";
  const sourceRows = rows.filter(isRowValue);

  switch (chart.chart_type) {
    case "pie":
      return renderPieChart(chart, sourceRows, compact, emptyLabel);
    case "bar":
      return renderBarChart(chart, sourceRows, compact, emptyLabel);
    case "area":
      return renderAreaChart(chart, sourceRows, compact, emptyLabel);
    case "scatter":
      return renderScatterChart(chart, sourceRows, compact, emptyLabel);
    case "radar":
      return renderRadarChart(chart, sourceRows, compact, emptyLabel);
    case "radial_bar":
      return renderRadialBarChart(chart, sourceRows, compact, emptyLabel);
    case "composed":
      return renderComposedChart(chart, sourceRows, compact, emptyLabel);
    case "treemap":
      return renderTreemapChart(chart, sourceRows, compact, emptyLabel);
    case "funnel":
      return renderFunnelChart(chart, sourceRows, compact, emptyLabel);
    case "sunburst":
      return renderSunburstChart(chart, sourceRows, compact, emptyLabel);
    case "line":
    default:
      return renderLineChart(chart, sourceRows, compact, emptyLabel);
  }
}

function renderLineChart(
  chart: DatapanelChartConfig,
  rows: RowValue[],
  compact: boolean,
  emptyLabel: string,
) {
  const xKey = chart.x_key;
  const yKeys = chart.y_keys ?? [];

  if (!xKey || yKeys.length === 0) {
    return <EmptyChart label={emptyLabel} />;
  }

  const data = metricRows(rows, xKey, yKeys);

  if (data.length === 0) {
    return <EmptyChart label={emptyLabel} />;
  }

  return (
    <ChartShell>
      <LineChart data={data} margin={chartMargin(compact)}>
        {cartesianAxes(xKey, compact)}
        {legend(compact)}
        {yKeys.map((key, index) => (
          <Line
            key={key}
            type="monotone"
            dataKey={key}
            stroke={seriesColor(index)}
            strokeWidth={compact ? 1.75 : 2}
            dot={false}
            connectNulls
            isAnimationActive={false}
          />
        ))}
      </LineChart>
    </ChartShell>
  );
}

function renderBarChart(
  chart: DatapanelChartConfig,
  rows: RowValue[],
  compact: boolean,
  emptyLabel: string,
) {
  const xKey = chart.x_key;
  const yKeys = chart.y_keys ?? [];

  if (!xKey || yKeys.length === 0) {
    return <EmptyChart label={emptyLabel} />;
  }

  const data = metricRows(rows, xKey, yKeys);

  if (data.length === 0) {
    return <EmptyChart label={emptyLabel} />;
  }

  return (
    <ChartShell>
      <BarChart data={data} margin={chartMargin(compact)}>
        {cartesianAxes(xKey, compact)}
        {legend(compact)}
        {yKeys.map((key, index) => (
          <Bar
            key={key}
            dataKey={key}
            fill={seriesColor(index)}
            radius={compact ? [3, 3, 0, 0] : [5, 5, 0, 0]}
            isAnimationActive={false}
          />
        ))}
      </BarChart>
    </ChartShell>
  );
}

function renderAreaChart(
  chart: DatapanelChartConfig,
  rows: RowValue[],
  compact: boolean,
  emptyLabel: string,
) {
  const xKey = chart.x_key;
  const yKeys = chart.y_keys ?? [];

  if (!xKey || yKeys.length === 0) {
    return <EmptyChart label={emptyLabel} />;
  }

  const data = metricRows(rows, xKey, yKeys);

  if (data.length === 0) {
    return <EmptyChart label={emptyLabel} />;
  }

  return (
    <ChartShell>
      <AreaChart data={data} margin={chartMargin(compact)}>
        {cartesianAxes(xKey, compact)}
        {legend(compact)}
        {yKeys.map((key, index) => (
          <Area
            key={key}
            type="monotone"
            dataKey={key}
            stroke={seriesColor(index)}
            fill={seriesColor(index)}
            fillOpacity={compact ? 0.12 : 0.16}
            strokeWidth={compact ? 1.75 : 2}
            connectNulls
            isAnimationActive={false}
          />
        ))}
      </AreaChart>
    </ChartShell>
  );
}

function renderPieChart(
  chart: DatapanelChartConfig,
  rows: RowValue[],
  compact: boolean,
  emptyLabel: string,
) {
  const xKey = chart.x_key;
  const valueKey = chart.y_keys?.[0];

  if (!xKey || !valueKey) {
    return <EmptyChart label={emptyLabel} />;
  }

  const data = categoricalValueRows(rows, xKey, valueKey);

  if (data.length === 0) {
    return <EmptyChart label={emptyLabel} />;
  }

  return (
    <ChartShell>
      <PieChart>
        {tooltip()}
        <Pie
          data={data}
          dataKey="value"
          nameKey="name"
          innerRadius={compact ? "48%" : "52%"}
          outerRadius={compact ? "76%" : "78%"}
          paddingAngle={compact ? 1 : 2}
          stroke="var(--background)"
          strokeWidth={compact ? 1 : 2}
          isAnimationActive={false}
        >
          {data.map((entry, index) => (
            <Cell key={entry.name} fill={seriesColor(index)} />
          ))}
        </Pie>
      </PieChart>
    </ChartShell>
  );
}

function renderScatterChart(
  chart: DatapanelChartConfig,
  rows: RowValue[],
  compact: boolean,
  emptyLabel: string,
) {
  const xKey = chart.x_key;
  const yKey = chart.y_keys?.[0];

  if (!xKey || !yKey) {
    return <EmptyChart label={emptyLabel} />;
  }

  const { rows: data, xAxis } = scatterRows(rows, xKey, yKey, chart.z_key);

  if (data.length === 0) {
    return <EmptyChart label={emptyLabel} />;
  }

  return (
    <ChartShell>
      <ScatterChart data={data} margin={chartMargin(compact)}>
        {scatterAxes(xKey, yKey, compact, xAxis)}
        {chart.z_key ? (
          <ZAxis dataKey={chart.z_key} range={compact ? [24, 72] : [40, 180]} />
        ) : (
          <ZAxis range={compact ? [36, 36] : [64, 64]} />
        )}
        <Scatter
          name={yKey}
          data={data}
          fill={seriesColor(0)}
          line={!compact && xAxis.mode !== "category"}
          lineType="fitting"
          isAnimationActive={false}
        />
      </ScatterChart>
    </ChartShell>
  );
}

function renderRadarChart(
  chart: DatapanelChartConfig,
  rows: RowValue[],
  compact: boolean,
  emptyLabel: string,
) {
  const xKey = chart.x_key;
  const yKeys = chart.y_keys ?? [];

  if (!xKey || yKeys.length === 0) {
    return <EmptyChart label={emptyLabel} />;
  }

  const data = metricRows(rows, xKey, yKeys, { positiveOnly: true });

  if (data.length === 0) {
    return <EmptyChart label={emptyLabel} />;
  }

  return (
    <ChartShell>
      <RadarChart data={data} outerRadius={compact ? "68%" : "74%"}>
        <PolarGrid stroke={chartBase.grid} strokeOpacity={0.55} />
        <PolarAngleAxis
          dataKey={xKey}
          tick={{ fill: chartBase.text, fontSize: compact ? 10 : 12 }}
        />
        <PolarRadiusAxis tick={false} axisLine={false} />
        {tooltip()}
        {legend(compact)}
        {yKeys.map((key, index) => (
          <Radar
            key={key}
            dataKey={key}
            stroke={seriesColor(index)}
            fill={seriesColor(index)}
            fillOpacity={compact ? 0.12 : 0.18}
            isAnimationActive={false}
          />
        ))}
      </RadarChart>
    </ChartShell>
  );
}

function renderRadialBarChart(
  chart: DatapanelChartConfig,
  rows: RowValue[],
  compact: boolean,
  emptyLabel: string,
) {
  const xKey = chart.x_key;
  const valueKey = chart.y_keys?.[0];

  if (!xKey || !valueKey) {
    return <EmptyChart label={emptyLabel} />;
  }

  const data = categoricalValueRows(rows, xKey, valueKey);

  if (data.length === 0) {
    return <EmptyChart label={emptyLabel} />;
  }

  return (
    <ChartShell>
      <RadialBarChart
        data={data}
        innerRadius={compact ? "18%" : "22%"}
        outerRadius={compact ? "86%" : "84%"}
        startAngle={90}
        endAngle={-270}
      >
        <PolarAngleAxis type="number" domain={[0, maxValue(data)]} tick={false} />
        {tooltip()}
        <RadialBar
          dataKey="value"
          background
          cornerRadius={compact ? 3 : 5}
          fill={seriesColor(0)}
          isAnimationActive={false}
        />
      </RadialBarChart>
    </ChartShell>
  );
}

function renderComposedChart(
  chart: DatapanelChartConfig,
  rows: RowValue[],
  compact: boolean,
  emptyLabel: string,
) {
  const xKey = chart.x_key;
  const series = chart.series ?? [];

  if (!xKey || series.length === 0) {
    return <EmptyChart label={emptyLabel} />;
  }

  const data = metricRows(
    rows,
    xKey,
    series.map((item) => item.key),
  );

  if (data.length === 0) {
    return <EmptyChart label={emptyLabel} />;
  }

  return (
    <ChartShell>
      <ComposedChart data={data} margin={chartMargin(compact)}>
        {cartesianAxes(xKey, compact)}
        {legend(compact)}
        {series.map((item, index) => renderComposedSeries(item, index, compact))}
      </ComposedChart>
    </ChartShell>
  );
}

function renderTreemapChart(
  chart: DatapanelChartConfig,
  rows: RowValue[],
  compact: boolean,
  emptyLabel: string,
) {
  const groupKeys = chart.group_keys ?? [];
  const valueKey = chart.value_key;

  if (groupKeys.length === 0 || !valueKey) {
    return <EmptyChart label={emptyLabel} />;
  }

  const root = hierarchyData(rows, groupKeys, valueKey);
  const data = root.children ?? [];

  if (data.length === 0) {
    return <EmptyChart label={emptyLabel} />;
  }

  return (
    <ChartShell>
      <Treemap
        data={data}
        dataKey="value"
        nameKey="name"
        type="nest"
        stroke="var(--background)"
        fill={seriesColor(0)}
        content={<TreemapTile compact={compact} />}
        nestIndexContent={() => null}
        isAnimationActive={false}
      >
        {tooltip()}
      </Treemap>
    </ChartShell>
  );
}

function renderFunnelChart(
  chart: DatapanelChartConfig,
  rows: RowValue[],
  compact: boolean,
  emptyLabel: string,
) {
  const xKey = chart.x_key;
  const valueKey = chart.y_keys?.[0];

  if (!xKey || !valueKey) {
    return <EmptyChart label={emptyLabel} />;
  }

  const data = categoricalValueRows(rows, xKey, valueKey);

  if (data.length === 0) {
    return <EmptyChart label={emptyLabel} />;
  }

  return (
    <ChartShell>
      <FunnelChart>
        {tooltip()}
        <Funnel data={data} dataKey="value" nameKey="name" isAnimationActive={false}>
          {data.map((entry, index) => (
            <Cell key={entry.name} fill={seriesColor(index)} />
          ))}
          {!compact ? <LabelList dataKey="name" position="right" /> : null}
        </Funnel>
      </FunnelChart>
    </ChartShell>
  );
}

function renderSunburstChart(
  chart: DatapanelChartConfig,
  rows: RowValue[],
  compact: boolean,
  emptyLabel: string,
) {
  const groupKeys = chart.group_keys ?? [];
  const valueKey = chart.value_key;

  if (groupKeys.length === 0 || !valueKey) {
    return <EmptyChart label={emptyLabel} />;
  }

  const data = hierarchyData(rows, groupKeys, valueKey);

  if (!data.children?.length) {
    return <EmptyChart label={emptyLabel} />;
  }

  return (
    <ChartShell>
      <SunburstChart
        data={data}
        dataKey="value"
        innerRadius={compact ? 10 : 16}
        ringPadding={compact ? 1 : 2}
        stroke="var(--background)"
        textOptions={{
          fill: chartBase.text,
          fontSize: compact ? "9px" : "11px",
          pointerEvents: "none",
        }}
      >
        {tooltip()}
      </SunburstChart>
    </ChartShell>
  );
}

function renderComposedSeries(
  item: DatapanelChartSeries,
  index: number,
  compact: boolean,
) {
  const color = seriesColor(index);

  if (item.kind === "bar") {
    return (
      <Bar
        key={`${item.kind}:${item.key}`}
        dataKey={item.key}
        fill={color}
        radius={compact ? [3, 3, 0, 0] : [5, 5, 0, 0]}
        isAnimationActive={false}
      />
    );
  }

  if (item.kind === "area") {
    return (
      <Area
        key={`${item.kind}:${item.key}`}
        type="monotone"
        dataKey={item.key}
        stroke={color}
        fill={color}
        fillOpacity={0.14}
        strokeWidth={compact ? 1.75 : 2}
        connectNulls
        isAnimationActive={false}
      />
    );
  }

  return (
    <Line
      key={`${item.kind}:${item.key}`}
      type="monotone"
      dataKey={item.key}
      stroke={color}
      strokeWidth={compact ? 1.75 : 2}
      dot={false}
      connectNulls
      isAnimationActive={false}
    />
  );
}

function scatterAxes(
  xKey: string,
  yKey: string,
  compact: boolean,
  xAxis: ScatterXAxis,
) {
  const isCategoryAxis = xAxis.mode === "category";

  return [
    <CartesianGrid
      key="grid"
      stroke={chartBase.grid}
      strokeOpacity={0.55}
      vertical={false}
    />,
    <XAxis
      key="x-axis"
      dataKey={xKey}
      type="number"
      domain={
        isCategoryAxis && xAxis.categories?.length
          ? [0, Math.max(0, xAxis.categories.length - 1)]
          : undefined
      }
      allowDecimals={!isCategoryAxis}
      tickFormatter={scatterXAxisTickFormatter(xAxis, compact)}
      tickLine={false}
      axisLine={false}
      tick={{ fill: chartBase.text, fontSize: compact ? 10 : 12 }}
    />,
    <YAxis
      key="y-axis"
      dataKey={yKey}
      type="number"
      tickLine={false}
      axisLine={false}
      tick={{ fill: chartBase.text, fontSize: compact ? 10 : 12 }}
      width={compact ? 34 : 42}
    />,
    tooltip(),
  ];
}

function cartesianAxes(xKey: string, compact: boolean, numericX = false) {
  return [
    <CartesianGrid
      key="grid"
      stroke={chartBase.grid}
      strokeOpacity={0.55}
      vertical={false}
    />,
    <XAxis
      key="x-axis"
      dataKey={xKey}
      type={numericX ? "number" : "category"}
      tickLine={false}
      axisLine={false}
      tick={{ fill: chartBase.text, fontSize: compact ? 10 : 12 }}
    />,
    <YAxis
      key="y-axis"
      tickLine={false}
      axisLine={false}
      tick={{ fill: chartBase.text, fontSize: compact ? 10 : 12 }}
      width={compact ? 34 : 42}
    />,
    tooltip(),
  ];
}

function tooltip() {
  return (
    <ChartTooltip
      key="tooltip"
      cursor={false}
      content={<ChartTooltipContent indicator="dot" />}
    />
  );
}

function legend(compact: boolean) {
  if (compact) {
    return null;
  }

  return (
    <ChartLegend
      verticalAlign="top"
      height={28}
      content={<ChartLegendContent />}
    />
  );
}

function ChartShell({
  children,
}: {
  children: ComponentProps<typeof ChartContainer>["children"];
}) {
  return (
    <ChartContainer config={chartConfig}>
      {children}
    </ChartContainer>
  );
}

function seriesColor(index: number) {
  return `var(--color-series-${index % chartTokens.length})`;
}

function chartMargin(compact: boolean) {
  return {
    top: compact ? 4 : 8,
    right: compact ? 12 : 24,
    bottom: 0,
    left: compact ? -12 : -6,
  };
}

function metricRows(
  rows: RowValue[],
  xKey: string,
  yKeys: string[],
  options: { positiveOnly?: boolean } = {},
): ChartRow[] {
  return rows.flatMap((row) => {
    const label = labelValue(row[xKey]);

    if (label === null) {
      return [];
    }

    const next: ChartRow = { [xKey]: label };
    let hasMetric = false;

    for (const key of yKeys) {
      const value = finiteNumber(row[key]);
      const validValue =
        value !== null && (!options.positiveOnly || value > 0) ? value : null;
      next[key] = validValue;
      hasMetric ||= validValue !== null;
    }

    return hasMetric ? [next] : [];
  });
}

function scatterRows(
  rows: RowValue[],
  xKey: string,
  yKey: string,
  zKey?: string,
): { rows: ChartRow[]; xAxis: ScatterXAxis } {
  const points = rows.flatMap((row): ScatterPoint[] => {
    const x = scatterXValue(row[xKey]);
    const y = finiteNumber(row[yKey]);

    if (x === null || y === null) {
      return [];
    }

    return [
      {
        x,
        y,
        z: zKey ? finiteNumber(row[zKey]) ?? 1 : null,
      },
    ];
  });

  if (points.length === 0) {
    return { rows: [], xAxis: { mode: "number" } };
  }

  const xAxis = scatterXAxis(points);
  const categoryIndexes = new Map<string, number>();
  const categories: string[] = [];

  const nextRows = points.map((point) => {
    const next: ChartRow = {
      [xKey]: scatterXCoordinate(point.x, xAxis, categoryIndexes, categories),
      [yKey]: point.y,
    };

    if (zKey && point.z !== null) {
      next[zKey] = point.z;
    }

    return next;
  });

  if (xAxis.mode === "category") {
    xAxis.categories = categories;
  }

  return { rows: nextRows, xAxis };
}

function categoricalValueRows(
  rows: RowValue[],
  nameKey: string,
  valueKey: string,
): Array<{ name: string; value: number; fill: string }> {
  return rows.flatMap((row, index) => {
    const name = labelValue(row[nameKey]);
    const value = finiteNumber(row[valueKey]);

    if (name === null || value === null || value <= 0) {
      return [];
    }

    return [
      {
        name: String(name),
        value,
        fill: seriesColor(index),
      },
    ];
  });
}

function hierarchyData(
  rows: RowValue[],
  groupKeys: string[],
  valueKey: string,
): HierarchyNode {
  const root: HierarchyNode = { name: "root", value: 0, children: [] };

  for (const row of rows) {
    const path = groupKeys
      .map((key) => labelValue(row[key]))
      .filter((value): value is string | number => value !== null)
      .map(String);
    const value = finiteNumber(row[valueKey]);

    if (path.length !== groupKeys.length || value === null || value <= 0) {
      continue;
    }

    addHierarchyValue(root, path, value);
  }

  assignHierarchyFills(root.children ?? [], 0);

  return root;
}

function addHierarchyValue(node: HierarchyNode, path: string[], value: number) {
  node.value = (node.value ?? 0) + value;

  if (path.length === 0) {
    return;
  }

  const [name, ...rest] = path;
  node.children ??= [];

  let child = node.children.find((item) => item.name === name);

  if (!child) {
    child = { name, value: 0, children: [] };
    node.children.push(child);
  }

  addHierarchyValue(child, rest, value);

  if (child.children?.length === 0) {
    delete child.children;
  }
}

function assignHierarchyFills(nodes: HierarchyNode[], depth: number) {
  nodes.forEach((node, index) => {
    node.fill = seriesColor(index + depth);

    if (node.children) {
      assignHierarchyFills(node.children, depth + index + 1);
    }
  });
}

function maxValue(data: Array<{ value: number }>) {
  return Math.max(1, ...data.map((item) => item.value));
}

function isRowValue(value: unknown): value is RowValue {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

function labelValue(value: unknown): string | number | null {
  if (typeof value === "string") {
    const trimmed = value.trim();
    return trimmed ? trimmed : null;
  }

  if (typeof value === "number" && Number.isFinite(value)) {
    return value;
  }

  return null;
}

function finiteNumber(value: unknown): number | null {
  if (typeof value === "number") {
    return Number.isFinite(value) ? value : null;
  }

  if (typeof value === "string" && value.trim()) {
    const parsed = Number(value);
    return Number.isFinite(parsed) ? parsed : null;
  }

  return null;
}

function scatterXValue(value: unknown): ScatterXValue | null {
  const numericValue = finiteNumber(value);

  if (numericValue !== null) {
    return { kind: "number", value: numericValue };
  }

  const label = labelValue(value);

  if (label === null) {
    return null;
  }

  const text = String(label);
  const timestamp = Date.parse(text);

  if (Number.isFinite(timestamp)) {
    return { kind: "time", label: text, value: timestamp };
  }

  return { kind: "category", label: text };
}

function scatterXAxis(points: ScatterPoint[]): ScatterXAxis {
  if (points.every((point) => point.x.kind === "number")) {
    return { mode: "number" };
  }

  if (points.every((point) => point.x.kind === "time")) {
    return { mode: "time" };
  }

  return { mode: "category" };
}

function scatterXCoordinate(
  value: ScatterXValue,
  xAxis: ScatterXAxis,
  categoryIndexes: Map<string, number>,
  categories: string[],
): number {
  if (xAxis.mode === "number" && value.kind === "number") {
    return value.value;
  }

  if (xAxis.mode === "time" && value.kind === "time") {
    return value.value;
  }

  const label = scatterXLabel(value);
  let index = categoryIndexes.get(label);

  if (index === undefined) {
    index = categories.length;
    categoryIndexes.set(label, index);
    categories.push(label);
  }

  return index;
}

function scatterXLabel(value: ScatterXValue): string {
  if (value.kind === "number") {
    return String(value.value);
  }

  return value.label;
}

function scatterXAxisTickFormatter(xAxis: ScatterXAxis, compact: boolean) {
  if (xAxis.mode === "time") {
    return (value: string | number) => {
      const timestamp = finiteNumber(value);
      return timestamp === null ? "" : formatScatterTimeTick(timestamp, compact);
    };
  }

  if (xAxis.mode === "category") {
    return (value: string | number) => {
      const index = finiteNumber(value);

      if (index === null || !Number.isInteger(index)) {
        return "";
      }

      return xAxis.categories?.[index] ?? "";
    };
  }

  return undefined;
}

function formatScatterTimeTick(timestamp: number, compact: boolean) {
  const date = new Date(timestamp);

  if (Number.isNaN(date.getTime())) {
    return "";
  }

  const datePart = date.toLocaleDateString(undefined, {
    month: "2-digit",
    day: "2-digit",
  });
  const timePart = date.toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
  });

  return compact ? timePart : `${datePart} ${timePart}`;
}

function EmptyChart({ label }: { label: string }) {
  return (
    <div className="flex h-full min-h-24 items-center justify-center rounded-sm border border-dashed bg-muted/20 px-3 text-center text-xs text-muted-foreground">
      {label}
    </div>
  );
}

function TreemapTile(props: {
  compact: boolean;
  x?: number;
  y?: number;
  width?: number;
  height?: number;
  name?: string;
  depth?: number;
  index?: number;
}) {
  const {
    compact,
    x = 0,
    y = 0,
    width = 0,
    height = 0,
    name,
    depth = 0,
    index = 0,
  } = props;

  if (width <= 0 || height <= 0) {
    return null;
  }

  const fill = seriesColor(index + depth);
  const showLabel = !compact && width >= 72 && height >= 28 && name;

  return (
    <g>
      <rect
        x={x}
        y={y}
        width={width}
        height={height}
        fill={fill}
        stroke="var(--background)"
        strokeWidth={2}
      />
      {showLabel ? (
        <text
          x={x + 8}
          y={y + 18}
          fill="var(--background)"
          fontSize={11}
          pointerEvents="none"
        >
          {name}
        </text>
      ) : null}
    </g>
  );
}
