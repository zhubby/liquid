"use client";

import { useEffect, useState } from "react";
import { Light as SyntaxHighlighter } from "react-syntax-highlighter";
import sqlSyntax from "react-syntax-highlighter/dist/esm/languages/hljs/sql";

import { cn } from "@/lib/utils";

const SQL_SAMPLES: readonly string[] = [
  `SELECT
  o.id,
  o.customer_id,
  o.total_amount,
  o.status
FROM orders o
WHERE o.created_at >= now() - interval '30 days'
  AND o.status IN ('paid', 'shipped')
ORDER BY o.total_amount DESC
LIMIT 100;`,
  `EXPLAIN (ANALYZE, BUFFERS)
SELECT
  c.email,
  count(*) AS failed_queries
FROM query_events q
JOIN customers c ON c.id = q.customer_id
WHERE q.risk_score >= 72
GROUP BY c.email
ORDER BY failed_queries DESC;`,
  `UPDATE orders
SET status = 'archived',
    archived_at = now()
WHERE tenant_id = $1
  AND created_at < now() - interval '18 months'
  AND status = 'closed'
RETURNING id, total_amount;`,
  `DELETE FROM user_sessions
WHERE last_seen_at < now() - interval '90 days'
  AND user_id IN (
    SELECT id
    FROM users
    WHERE disabled_at IS NOT NULL
  );`,
  `INSERT INTO audit_log (
  actor_id,
  statement_kind,
  risk_score,
  approved_at
)
SELECT $1, 'select', 18, now()
WHERE EXISTS (
  SELECT 1 FROM managed_databases WHERE id = $2
);`,
];

const TYPE_DELAY_MS = 24;
const TYPE_JITTER_MS = 18;
const HOLD_DELAY_MS = 1700;
const CLEAR_DELAY_MS = 160;
const NEXT_DELAY_MS = 360;

type TerminalPhase = "typing" | "clearing" | "between";

SyntaxHighlighter.registerLanguage("sql", sqlSyntax);

export function AuthSqlTerminal({ label }: { label: string }) {
  const [sampleIndex, setSampleIndex] = useState(0);
  const [visibleSql, setVisibleSql] = useState("");
  const [phase, setPhase] = useState<TerminalPhase>("typing");
  const [reducedMotion, setReducedMotion] = useState(false);

  useEffect(() => {
    const media = window.matchMedia("(prefers-reduced-motion: reduce)");
    const updateReducedMotion = () => {
      setReducedMotion(media.matches);
    };

    updateReducedMotion();
    media.addEventListener("change", updateReducedMotion);

    return () => media.removeEventListener("change", updateReducedMotion);
  }, []);

  useEffect(() => {
    if (reducedMotion) {
      setSampleIndex(0);
      setVisibleSql(SQL_SAMPLES[0]);
      setPhase("typing");
      return;
    }

    const sql = SQL_SAMPLES[sampleIndex];
    let timeout: ReturnType<typeof setTimeout>;

    if (phase === "typing" && visibleSql.length < sql.length) {
      timeout = setTimeout(() => {
        setVisibleSql(sql.slice(0, visibleSql.length + 1));
      }, TYPE_DELAY_MS + Math.floor(Math.random() * TYPE_JITTER_MS));

      return () => clearTimeout(timeout);
    }

    if (phase === "typing") {
      timeout = setTimeout(() => setPhase("clearing"), HOLD_DELAY_MS);
      return () => clearTimeout(timeout);
    }

    if (phase === "clearing") {
      timeout = setTimeout(() => {
        setVisibleSql("");
        setPhase("between");
      }, CLEAR_DELAY_MS);

      return () => clearTimeout(timeout);
    }

    timeout = setTimeout(() => {
      setSampleIndex((currentIndex) => getNextSampleIndex(currentIndex));
      setPhase("typing");
    }, NEXT_DELAY_MS);

    return () => clearTimeout(timeout);
  }, [phase, reducedMotion, sampleIndex, visibleSql]);

  return (
    <div
      role="img"
      aria-label={label}
      className="flex h-full min-h-[260px] w-full flex-col overflow-hidden bg-muted/35 text-foreground dark:bg-background sm:min-h-[340px]"
    >
      <div
        aria-hidden
        className="flex h-10 shrink-0 items-center justify-between border-b bg-muted/80 px-3 dark:bg-muted/40"
      >
        <div className="flex w-16 items-center gap-1.5">
          <span className="size-2 rounded-full bg-destructive/70" />
          <span className="size-2 rounded-full bg-chart-5/70" />
          <span className="size-2 rounded-full bg-chart-2/70" />
        </div>
        <span className="font-mono text-[11px] font-medium text-muted-foreground">
          psql
        </span>
        <div className="w-16" />
      </div>

      <div
        aria-hidden
        className="flex min-h-0 flex-1 overflow-hidden bg-muted/30 p-3 dark:bg-background sm:p-4"
      >
        <div
          className={cn(
            "flex min-h-0 w-full overflow-hidden rounded-md border bg-background p-3 shadow-inner transition-opacity duration-150 dark:bg-card sm:p-4",
            phase === "clearing" && "opacity-0",
          )}
        >
          <span className="shrink-0 pr-3 font-mono text-[11px] font-semibold leading-5 text-chart-2 sm:text-xs">
            psql&gt;
          </span>
          <div className="min-w-0 flex-1 overflow-hidden">
            <SyntaxHighlighter
              language="sql"
              useInlineStyles={false}
              wrapLongLines
              className="liquid-code-highlight max-h-full overflow-hidden p-0 text-[11px] leading-5 sm:text-xs lg:text-[13px]"
              codeTagProps={{ className: "font-mono" }}
              customStyle={{
                margin: 0,
                background: "transparent",
                padding: 0,
              }}
            >
              {visibleSql}
            </SyntaxHighlighter>
            <span className="mt-1 inline-block h-4 w-2 animate-pulse bg-chart-2 motion-reduce:animate-none" />
          </div>
        </div>
      </div>
    </div>
  );
}

function getNextSampleIndex(currentIndex: number) {
  if (SQL_SAMPLES.length === 1) {
    return currentIndex;
  }

  let nextIndex = currentIndex;

  while (nextIndex === currentIndex) {
    nextIndex = Math.floor(Math.random() * SQL_SAMPLES.length);
  }

  return nextIndex;
}
