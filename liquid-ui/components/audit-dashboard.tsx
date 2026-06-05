"use client";

import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
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
  low: "var(--chart-2)",
  medium: "var(--chart-4)",
  high: "var(--chart-5)",
  critical: "var(--destructive)",
};

const chartColors = {
  audited: "var(--chart-2)",
  flagged: "var(--destructive)",
  grid: "var(--border)",
  text: "var(--muted-foreground)",
  tooltipBackground: "var(--popover)",
  tooltipBorder: "var(--border)",
  tooltipText: "var(--popover-foreground)",
};

const apiBaseUrl =
  process.env.NEXT_PUBLIC_API_BASE_URL ?? "http://localhost:3001";

export function AuditDashboard() {
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

  const flaggedRate = useMemo(
    () =>
      summary.total_queries === 0
        ? 0
        : (summary.flagged_queries / summary.total_queries) * 100,
    [summary.flagged_queries, summary.total_queries],
  );

  return (
    <main className="min-h-screen bg-background px-4 py-4 text-foreground sm:px-6 lg:px-8">
      <div className="mx-auto flex max-w-7xl flex-col gap-5 pb-8">
        <header className="sticky top-4 z-20">
          <Card className="py-4">
            <CardContent className="flex flex-col gap-3 px-4 sm:flex-row sm:items-center sm:justify-between sm:px-5">
              <DashboardHeaderContent
                handleRefresh={handleRefresh}
                status={status}
              />
            </CardContent>
          </Card>
        </header>

        <section className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
          <MetricCard
            icon={<Database className="size-5" aria-hidden />}
            label="Audited queries"
            value={summary.total_queries.toLocaleString()}
            tone="primary"
          />
          <MetricCard
            icon={<AlertTriangle className="size-5" aria-hidden />}
            label="Flagged queries"
            value={summary.flagged_queries.toLocaleString()}
            detail={`${flaggedRate.toFixed(1)}% flag rate`}
            tone="warning"
          />
          <MetricCard
            icon={<Gauge className="size-5" aria-hidden />}
            label="Audit score"
            value={`${summary.audit_score}/100`}
            tone="success"
          />
          <MetricCard
            icon={<Clock3 className="size-5" aria-hidden />}
            label="Average latency"
            value={`${summary.average_latency_ms.toFixed(1)} ms`}
            detail={`${summary.high_risk_queries} high risk`}
            tone="danger"
          />
        </section>

        <section className="grid gap-4 xl:grid-cols-[1.6fr_1fr]">
          <Card>
            <CardHeader>
              <CardTitle className="text-base">Audit Volume</CardTitle>
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
                        borderRadius: "var(--radius-md)",
                        background: chartColors.tooltipBackground,
                        borderColor: chartColors.tooltipBorder,
                        boxShadow: "0 1px 2px rgb(0 0 0 / 0.05)",
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
          </Card>

          <Card>
            <CardHeader>
              <CardTitle className="text-base">Risk Breakdown</CardTitle>
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
                        borderRadius: "var(--radius-md)",
                        background: chartColors.tooltipBackground,
                        borderColor: chartColors.tooltipBorder,
                        boxShadow: "0 1px 2px rgb(0 0 0 / 0.05)",
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
          </Card>
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
    <>
      <div className="flex min-w-0 items-center gap-3">
        <div className="inline-flex size-9 shrink-0 items-center justify-center rounded-md border bg-primary text-primary-foreground">
          <ShieldCheck className="size-5" aria-hidden />
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
        <div className="inline-flex h-9 items-center gap-2 rounded-md border bg-background px-3 text-sm text-muted-foreground shadow-xs">
          <CalendarDays className="size-4 text-foreground" aria-hidden />
          Last 7 days
        </div>
        <StatusBadge status={status} />
        <Button
          variant="outline"
          size="icon"
          onClick={handleRefresh}
          aria-label="Refresh audit summary"
        >
          <RefreshCcw
            className={`size-4 ${status === "loading" ? "animate-spin" : ""}`}
            aria-hidden
          />
        </Button>
      </div>
    </>
  );
}

function StatusBadge({
  status,
}: {
  status: "loading" | "live" | "offline";
}) {
  const statusConfig = {
    live: { label: "Live API", variant: "secondary" as const },
    loading: { label: "Loading", variant: "outline" as const },
    offline: { label: "Mock data", variant: "destructive" as const },
  }[status];

  return (
    <Badge
      variant={statusConfig.variant}
      className="h-7 gap-1.5 rounded-md px-2.5"
    >
      <span className="size-1.5 rounded-full bg-current" aria-hidden />
      {statusConfig.label}
    </Badge>
  );
}

type MetricCardProps = {
  icon: ReactNode;
  label: string;
  value: string;
  detail?: string;
  tone: "primary" | "warning" | "success" | "danger";
};

function MetricCard({ icon, label, value, detail, tone }: MetricCardProps) {
  const toneClasses = {
    primary: "border-primary/20 bg-primary/10 text-primary",
    warning: "border-chart-4/30 bg-chart-4/15 text-foreground",
    success: "border-chart-2/30 bg-chart-2/15 text-foreground",
    danger: "border-destructive/25 bg-destructive/10 text-destructive",
  }[tone];

  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between gap-4">
        <CardTitle className="text-sm font-medium text-muted-foreground">
          {label}
        </CardTitle>
        <div
          className={`inline-flex size-9 shrink-0 items-center justify-center rounded-md border ${toneClasses}`}
        >
          {icon}
        </div>
      </CardHeader>
      <CardContent>
        <div className="text-3xl font-semibold tracking-normal">{value}</div>
        {detail ? (
          <div className="mt-2 text-sm text-muted-foreground">{detail}</div>
        ) : null}
      </CardContent>
    </Card>
  );
}
