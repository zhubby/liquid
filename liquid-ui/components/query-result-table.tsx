import type { BiQueryResult } from "@/lib/api";
import { cn } from "@/lib/utils";

type QueryResultTableProps = {
  result: BiQueryResult;
  emptyLabel: string;
  className?: string;
  maxRows?: number;
  stickyHeader?: boolean;
};

type QueryResultRow = Record<string, unknown>;

export function QueryResultTable({
  result,
  emptyLabel,
  className,
  maxRows,
  stickyHeader = true,
}: QueryResultTableProps) {
  const rows = result.rows as QueryResultRow[];
  const visibleRows =
    typeof maxRows === "number" ? rows.slice(0, maxRows) : rows;

  if (result.columns.length === 0 || visibleRows.length === 0) {
    return (
      <div
        className={cn(
          "flex h-full min-h-24 items-center justify-center rounded-md border bg-muted/20 px-3 py-6 text-center text-xs text-muted-foreground",
          className,
        )}
      >
        {emptyLabel}
      </div>
    );
  }

  return (
    <div className={cn("h-full overflow-auto rounded-md border", className)}>
      <table className="w-full min-w-max border-collapse text-xs">
        <thead
          className={cn(
            "bg-muted text-left text-muted-foreground",
            stickyHeader && "sticky top-0 z-10",
          )}
        >
          <tr className="border-b">
            {result.columns.map((column) => (
              <th key={column} className="px-2.5 py-2 font-medium">
                {column}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {visibleRows.map((row, index) => (
            <tr key={index} className="border-b last:border-0 hover:bg-muted/40">
              {result.columns.map((column) => (
                <td
                  key={column}
                  className="max-w-[28rem] px-2.5 py-2 align-top"
                >
                  <span className="break-words">
                    {formatQueryResultValue(row[column])}
                  </span>
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export function formatQueryResultValue(value: unknown) {
  if (value === null || value === undefined) {
    return "";
  }

  if (typeof value === "object") {
    return JSON.stringify(value);
  }

  return String(value);
}
