"use client";

import { useCallback, useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
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
  low: "#4d7c57",
  medium: "#d19a35",
  high: "#c85d38",
  critical: "#a83232",
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
    <main className="min-h-screen px-4 py-6 sm:px-6 lg:px-8">
      <div className="mx-auto flex max-w-7xl flex-col gap-6">
        <header className="flex flex-col gap-4 border-b pb-5 sm:flex-row sm:items-center sm:justify-between">
          <div>
            <div className="flex items-center gap-2">
              <ShieldCheck className="h-7 w-7 text-primary" aria-hidden />
              <h1 className="text-2xl font-semibold tracking-normal">
                Liquid SQL Audit
              </h1>
            </div>
            <p className="mt-2 max-w-3xl text-sm text-muted-foreground">
              AI audit telemetry and BI controls for SQL governance.
            </p>
          </div>
          <div className="flex items-center gap-2">
            <Badge
              className={
                status === "live"
                  ? "border-primary/30 bg-primary/10 text-primary"
                  : "border-secondary/40 bg-secondary/20 text-secondary-foreground"
              }
            >
              {status === "live"
                ? "Live API"
                : status === "loading"
                  ? "Loading"
                  : "Mock data"}
            </Badge>
            <Button variant="outline" onClick={handleRefresh}>
              <RefreshCcw className="h-4 w-4" aria-hidden />
              Refresh
            </Button>
          </div>
        </header>

        <section className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
          <MetricCard
            icon={<Database className="h-5 w-5" aria-hidden />}
            label="Audited queries"
            value={summary.total_queries.toLocaleString()}
            tone="primary"
          />
          <MetricCard
            icon={<AlertTriangle className="h-5 w-5" aria-hidden />}
            label="Flagged queries"
            value={summary.flagged_queries.toLocaleString()}
            detail={`${flaggedRate.toFixed(1)}% flag rate`}
            tone="warning"
          />
          <MetricCard
            icon={<Gauge className="h-5 w-5" aria-hidden />}
            label="Audit score"
            value={`${summary.audit_score}/100`}
            tone="success"
          />
          <MetricCard
            icon={<Clock3 className="h-5 w-5" aria-hidden />}
            label="Average latency"
            value={`${summary.average_latency_ms.toFixed(1)} ms`}
            detail={`${summary.high_risk_queries} high risk`}
            tone="danger"
          />
        </section>

        <section className="grid gap-4 xl:grid-cols-[1.6fr_1fr]">
          <Card>
            <CardHeader>
              <CardTitle>Audit Volume</CardTitle>
            </CardHeader>
            <CardContent>
              <div className="h-72">
                <ResponsiveContainer width="100%" height="100%">
                  <LineChart data={summary.trend}>
                    <CartesianGrid stroke="#ded7cc" vertical={false} />
                    <XAxis
                      dataKey="day"
                      tickLine={false}
                      axisLine={false}
                      tick={{ fill: "#6e675e", fontSize: 12 }}
                    />
                    <YAxis
                      tickLine={false}
                      axisLine={false}
                      tick={{ fill: "#6e675e", fontSize: 12 }}
                      width={48}
                    />
                    <Tooltip
                      contentStyle={{
                        borderRadius: 8,
                        borderColor: "#ded7cc",
                        color: "#1d1b18",
                      }}
                    />
                    <Line
                      type="monotone"
                      dataKey="audited"
                      stroke="#2f5d50"
                      strokeWidth={3}
                      dot={false}
                    />
                    <Line
                      type="monotone"
                      dataKey="flagged"
                      stroke="#b6403a"
                      strokeWidth={3}
                      dot={false}
                    />
                  </LineChart>
                </ResponsiveContainer>
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>Risk Breakdown</CardTitle>
            </CardHeader>
            <CardContent>
              <div className="h-72">
                <ResponsiveContainer width="100%" height="100%">
                  <BarChart data={summary.risk_breakdown} layout="vertical">
                    <CartesianGrid stroke="#ded7cc" horizontal={false} />
                    <XAxis
                      type="number"
                      tickLine={false}
                      axisLine={false}
                      tick={{ fill: "#6e675e", fontSize: 12 }}
                    />
                    <YAxis
                      dataKey="label"
                      type="category"
                      width={118}
                      tickLine={false}
                      axisLine={false}
                      tick={{ fill: "#6e675e", fontSize: 12 }}
                    />
                    <Tooltip
                      contentStyle={{
                        borderRadius: 8,
                        borderColor: "#ded7cc",
                        color: "#1d1b18",
                      }}
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

type MetricCardProps = {
  icon: React.ReactNode;
  label: string;
  value: string;
  detail?: string;
  tone: "primary" | "warning" | "success" | "danger";
};

function MetricCard({ icon, label, value, detail, tone }: MetricCardProps) {
  const tones = {
    primary: "bg-primary/10 text-primary",
    warning: "bg-secondary/30 text-secondary-foreground",
    success: "bg-emerald-100 text-emerald-800",
    danger: "bg-red-100 text-red-800",
  };

  return (
    <Card>
      <CardHeader className="flex-row items-center justify-between gap-4">
        <CardTitle>{label}</CardTitle>
        <div className={`rounded-md p-2 ${tones[tone]}`}>{icon}</div>
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
