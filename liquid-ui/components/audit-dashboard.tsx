"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import LiquidGlass from "liquid-glass-react";
import {
  AlertTriangle,
  CalendarDays,
  Clock3,
  Database,
  Gauge,
  RefreshCcw,
  ShieldCheck,
} from "lucide-react";
import {
  Bar,
  BarChart,
  CartesianGrid,
  Cell,
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";

type RiskSeverity = "low" | "medium" | "high" | "critical";

type RiskBreakdown = {
  label: string;
  count: number;
  severity: RiskSeverity;
};

type AuditTrendPoint = {
  day: string;
  audited: number;
  flagged: number;
};

type AuditSummary = {
  total_queries: number;
  flagged_queries: number;
  high_risk_queries: number;
  average_latency_ms: number;
  audit_score: number;
  risk_breakdown: RiskBreakdown[];
  trend: AuditTrendPoint[];
};

const fallbackSummary: AuditSummary = {
  total_queries: 12846,
  flagged_queries: 438,
  high_risk_queries: 37,
  average_latency_ms: 86.4,
  audit_score: 92,
  risk_breakdown: [
    { label: "PII exposure", count: 144, severity: "high" },
    { label: "Cartesian joins", count: 96, severity: "medium" },
    { label: "DDL mutation", count: 31, severity: "critical" },
    { label: "Unbounded scans", count: 167, severity: "low" },
  ],
  trend: [
    { day: "Mon", audited: 1840, flagged: 68 },
    { day: "Tue", audited: 1935, flagged: 71 },
    { day: "Wed", audited: 2018, flagged: 82 },
    { day: "Thu", audited: 1762, flagged: 49 },
    { day: "Fri", audited: 2114, flagged: 76 },
    { day: "Sat", audited: 1588, flagged: 44 },
    { day: "Sun", audited: 1589, flagged: 48 },
  ],
};

const riskColors: Record<RiskSeverity, string> = {
  low: "var(--accent-mint)",
  medium: "var(--warning)",
  high: "var(--danger)",
  critical: "var(--critical)",
};

const chartColors = {
  audited: "var(--primary)",
  flagged: "var(--danger)",
  grid: "var(--border)",
  text: "var(--muted-foreground)",
  tooltipBackground: "var(--glass-fill-strong)",
  tooltipBorder: "var(--glass-border)",
  tooltipText: "var(--foreground)",
};

const apiBaseUrl =
  process.env.NEXT_PUBLIC_API_BASE_URL ?? "http://localhost:3001";

export function AuditDashboard() {
  const [nativeGlassReady, setNativeGlassReady] = useState(false);
  const [summary, setSummary] = useState<AuditSummary>(fallbackSummary);
  const [status, setStatus] = useState<"loading" | "live" | "offline">(
    "loading",
  );

  const loadSummary = useCallback(async () => {
    try {
      const response = await fetch(`${apiBaseUrl}/api/v1/audit/summary`, {
        headers: { Accept: "application/json" },
      });

      if (!response.ok) {
        throw new Error(`API returned ${response.status}`);
      }

      setSummary(await response.json());
      setStatus("live");
    } catch {
      setSummary(fallbackSummary);
      setStatus("offline");
    }
  }, []);

  const handleRefresh = useCallback(() => {
    setStatus("loading");
    void loadSummary();
  }, [loadSummary]);

  useEffect(() => {
    void loadSummary();
  }, [loadSummary]);

  useEffect(() => {
    setNativeGlassReady(true);
  }, []);

  const flaggedRate = useMemo(
    () =>
      summary.total_queries === 0
        ? 0
        : (summary.flagged_queries / summary.total_queries) * 100,
    [summary.flagged_queries, summary.total_queries],
  );

  return (
    <main className="liquid-environment min-h-screen px-4 py-4 sm:px-6 lg:px-8">
      <div className="mx-auto flex max-w-7xl flex-col gap-5 pb-8">
        <header className="sticky top-4 z-20 rounded-2xl">
          <div
            className="invisible flex flex-col gap-3 px-4 py-3 sm:flex-row sm:items-center sm:justify-between"
            aria-hidden
          >
            <DashboardHeaderContent
              handleRefresh={handleRefresh}
              status={status}
            />
          </div>

          {nativeGlassReady ? (
            <LiquidGlass
              className="native-liquid-header w-full"
              style={{
                position: "absolute",
                top: "50%",
                left: "50%",
                width: "100%",
              }}
              padding="12px 16px"
              cornerRadius={20}
              displacementScale={46}
              blurAmount={0.03}
              saturation={155}
              aberrationIntensity={1.8}
              elasticity={0.18}
              mode="prominent"
            >
              <DashboardHeaderContent
                handleRefresh={handleRefresh}
                status={status}
              />
            </LiquidGlass>
          ) : (
            <div className="liquid-bar absolute inset-0 rounded-2xl px-4 py-3">
              <DashboardHeaderContent
                handleRefresh={handleRefresh}
                status={status}
              />
            </div>
          )}
        </header>

        <section className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
          <MetricCard
            icon={<Database className="h-5 w-5" aria-hidden />}
            label="Audited queries"
            value={summary.total_queries.toLocaleString()}
            tone="primary"
            nativeGlassReady={nativeGlassReady}
          />
          <MetricCard
            icon={<AlertTriangle className="h-5 w-5" aria-hidden />}
            label="Flagged queries"
            value={summary.flagged_queries.toLocaleString()}
            detail={`${flaggedRate.toFixed(1)}% flag rate`}
            tone="warning"
            nativeGlassReady={nativeGlassReady}
          />
          <MetricCard
            icon={<Gauge className="h-5 w-5" aria-hidden />}
            label="Audit score"
            value={`${summary.audit_score}/100`}
            tone="success"
            nativeGlassReady={nativeGlassReady}
          />
          <MetricCard
            icon={<Clock3 className="h-5 w-5" aria-hidden />}
            label="Average latency"
            value={`${summary.average_latency_ms.toFixed(1)} ms`}
            detail={`${summary.high_risk_queries} high risk`}
            tone="danger"
            nativeGlassReady={nativeGlassReady}
          />
        </section>

        <section className="grid gap-4 xl:grid-cols-[1.6fr_1fr]">
          <NativeGlassCard nativeGlassReady={nativeGlassReady}>
            <CardHeader>
              <CardTitle className="text-foreground">Audit Volume</CardTitle>
            </CardHeader>
            <CardContent>
              <div className="h-72">
                <ResponsiveContainer width="100%" height="100%">
                  <LineChart data={summary.trend}>
                    <CartesianGrid stroke={chartColors.grid} vertical={false} />
                    <XAxis
                      dataKey="day"
                      tickLine={false}
                      axisLine={false}
                      tick={{ fill: chartColors.text, fontSize: 12 }}
                    />
                    <YAxis
                      tickLine={false}
                      axisLine={false}
                      tick={{ fill: chartColors.text, fontSize: 12 }}
                      width={48}
                    />
                    <Tooltip
                      contentStyle={{
                        borderRadius: 8,
                        background: chartColors.tooltipBackground,
                        borderColor: chartColors.tooltipBorder,
                        boxShadow: "var(--glass-shadow)",
                        color: chartColors.tooltipText,
                      }}
                      labelStyle={{ color: chartColors.tooltipText }}
                      itemStyle={{ color: chartColors.tooltipText }}
                    />
                    <Line
                      type="monotone"
                      dataKey="audited"
                      stroke={chartColors.audited}
                      strokeWidth={2.5}
                      dot={false}
                    />
                    <Line
                      type="monotone"
                      dataKey="flagged"
                      stroke={chartColors.flagged}
                      strokeWidth={2.5}
                      dot={false}
                    />
                  </LineChart>
                </ResponsiveContainer>
              </div>
            </CardContent>
          </NativeGlassCard>

          <NativeGlassCard nativeGlassReady={nativeGlassReady}>
            <CardHeader>
              <CardTitle className="text-foreground">Risk Breakdown</CardTitle>
            </CardHeader>
            <CardContent>
              <div className="h-72">
                <ResponsiveContainer width="100%" height="100%">
                  <BarChart data={summary.risk_breakdown} layout="vertical">
                    <CartesianGrid stroke={chartColors.grid} horizontal={false} />
                    <XAxis
                      type="number"
                      tickLine={false}
                      axisLine={false}
                      tick={{ fill: chartColors.text, fontSize: 12 }}
                    />
                    <YAxis
                      dataKey="label"
                      type="category"
                      width={118}
                      tickLine={false}
                      axisLine={false}
                      tick={{ fill: chartColors.text, fontSize: 12 }}
                    />
                    <Tooltip
                      contentStyle={{
                        borderRadius: 8,
                        background: chartColors.tooltipBackground,
                        borderColor: chartColors.tooltipBorder,
                        boxShadow: "var(--glass-shadow)",
                        color: chartColors.tooltipText,
                      }}
                      labelStyle={{ color: chartColors.tooltipText }}
                      itemStyle={{ color: chartColors.tooltipText }}
                    />
                    <Bar dataKey="count" radius={[0, 6, 6, 0]}>
                      {summary.risk_breakdown.map((entry) => (
                        <Cell
                          key={entry.label}
                          fill={riskColors[entry.severity]}
                        />
                      ))}
                    </Bar>
                  </BarChart>
                </ResponsiveContainer>
              </div>
            </CardContent>
          </NativeGlassCard>
        </section>
      </div>
    </main>
  );
}

function DashboardHeaderContent({
  handleRefresh,
  status,
}: {
  handleRefresh: () => void;
  status: "loading" | "live" | "offline";
}) {
  return (
    <div className="flex w-full flex-col gap-3 text-foreground sm:flex-row sm:items-center sm:justify-between">
      <div className="flex min-w-0 items-center gap-3">
        <div className="metric-icon metric-primary shrink-0">
          <ShieldCheck className="h-5 w-5" aria-hidden />
        </div>
        <div className="min-w-0">
          <h1 className="truncate text-2xl font-semibold tracking-normal">
            Liquid SQL Audit
          </h1>
          <p className="truncate text-sm text-muted-foreground">
            SQL governance telemetry and BI controls
          </p>
        </div>
      </div>
      <div className="flex flex-wrap items-center gap-2">
        <div className="liquid-button inline-flex h-9 items-center gap-2 rounded-full px-3 text-sm text-muted-foreground">
          <CalendarDays className="h-4 w-4 text-primary" aria-hidden />
          Last 7 days
        </div>
        <StatusBadge status={status} />
        <Button
          variant="liquid"
          size="icon"
          onClick={handleRefresh}
          aria-label="Refresh audit summary"
        >
          <RefreshCcw
            className={`h-4 w-4 ${status === "loading" ? "animate-spin" : ""}`}
            aria-hidden
          />
        </Button>
      </div>
    </div>
  );
}

function StatusBadge({
  status,
}: {
  status: "loading" | "live" | "offline";
}) {
  const statusClass = {
    live: "status-live",
    loading: "status-loading",
    offline: "status-offline",
  }[status];

  return (
    <Badge className={`status-badge ${statusClass}`}>
      <span className="h-1.5 w-1.5 rounded-full bg-current" aria-hidden />
      {status === "live"
        ? "Live API"
        : status === "loading"
          ? "Loading"
          : "Mock data"}
    </Badge>
  );
}

type MetricCardProps = {
  icon: React.ReactNode;
  label: string;
  value: string;
  detail?: string;
  tone: "primary" | "warning" | "success" | "danger";
  nativeGlassReady: boolean;
};

function MetricCard({
  icon,
  label,
  value,
  detail,
  tone,
  nativeGlassReady,
}: MetricCardProps) {
  const tones = {
    primary: "metric-primary",
    warning: "metric-warning",
    success: "metric-success",
    danger: "metric-danger",
  };

  return (
    <NativeGlassCard nativeGlassReady={nativeGlassReady}>
      <CardHeader className="flex-row items-center justify-between gap-4">
        <CardTitle>{label}</CardTitle>
        <div className={`metric-icon ${tones[tone]}`}>{icon}</div>
      </CardHeader>
      <CardContent>
        <div className="text-3xl font-semibold tracking-normal">{value}</div>
        {detail ? (
          <div className="mt-2 text-sm text-muted-foreground">{detail}</div>
        ) : null}
      </CardContent>
    </NativeGlassCard>
  );
}

function NativeGlassCard({
  children,
  nativeGlassReady,
}: {
  children: React.ReactNode;
  nativeGlassReady: boolean;
}) {
  const containerRef = useRef<HTMLDivElement>(null);

  return (
    <div ref={containerRef} className="native-glass-card-shell relative">
      {nativeGlassReady ? (
        <LiquidGlass
          className="native-liquid-card pointer-events-none absolute h-full w-full"
          style={{
            position: "absolute",
            top: "50%",
            left: "50%",
            width: "100%",
            height: "100%",
          }}
          mouseContainer={containerRef}
          padding="0"
          cornerRadius={8}
          displacementScale={32}
          blurAmount={0.025}
          saturation={150}
          aberrationIntensity={1.35}
          elasticity={0.08}
          mode="standard"
        >
          <span aria-hidden />
        </LiquidGlass>
      ) : null}
      <Card className="glass-data-card relative z-10">{children}</Card>
    </div>
  );
}
