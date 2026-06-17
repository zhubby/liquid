"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import {
  AlertCircle,
  CalendarDays,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  ClipboardCheck,
  Copy,
  FileText,
  Loader2,
  RefreshCw,
  RotateCcw,
  X,
} from "lucide-react";
import { toast } from "sonner";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  type ManagedDatabase,
  type RiskSeverity,
  type SqlAuditExecutionStatus,
  type SqlAuditLifecycleStatus,
  type SqlAuditRecord,
  type SqlRollbackPlan,
  type SqlAuditStatus,
  apiRequest,
  apiRequestWithMeta,
} from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

type SqlAuditHistoryProps = {
  token: string;
  databases: ManagedDatabase[];
};

const pageSizeOptions = [10, 20, 50, 100] as const;

type PageSize = (typeof pageSizeOptions)[number];
type FilterValue<T extends string> = "all" | T;

export function SqlAuditHistory({ token, databases }: SqlAuditHistoryProps) {
  const { locale, t } = useI18n();
  const [records, setRecords] = useState<SqlAuditRecord[]>([]);
  const [totalCount, setTotalCount] = useState(0);
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState<PageSize>(20);
  const [databaseId, setDatabaseId] = useState("all");
  const [auditStatus, setAuditStatus] =
    useState<FilterValue<SqlAuditLifecycleStatus>>("all");
  const [executionStatus, setExecutionStatus] =
    useState<FilterValue<SqlAuditExecutionStatus>>("all");
  const [createdFrom, setCreatedFrom] = useState("");
  const [createdTo, setCreatedTo] = useState("");
  const [refreshKey, setRefreshKey] = useState(0);
  const [isLoading, setIsLoading] = useState(true);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [selectedRecord, setSelectedRecord] = useState<SqlAuditRecord | null>(
    null,
  );
  const [isDetailLoading, setIsDetailLoading] = useState(false);

  const pageCount = Math.max(1, Math.ceil(totalCount / pageSize));

  const queryPath = useMemo(() => {
    const params = new URLSearchParams({
      page: String(page),
      page_size: String(pageSize),
    });

    if (databaseId !== "all") {
      params.set("managed_database_id", databaseId);
    }

    if (auditStatus !== "all") {
      params.set("audit_status", auditStatus);
    }

    if (executionStatus !== "all") {
      params.set("execution_status", executionStatus);
    }

    const fromBoundary = localDateBoundary(createdFrom, "start");
    const toBoundary = localDateBoundary(createdTo, "end");

    if (fromBoundary) {
      params.set("created_from", fromBoundary);
    }

    if (toBoundary) {
      params.set("created_to", toBoundary);
    }

    return `/api/v1/sql-audits?${params.toString()}`;
  }, [
    auditStatus,
    createdFrom,
    createdTo,
    databaseId,
    executionStatus,
    page,
    pageSize,
  ]);

  useEffect(() => {
    let cancelled = false;

    const loadRecords = async () => {
      setIsLoading(true);

      try {
        const response = await apiRequestWithMeta<SqlAuditRecord[]>(queryPath, {
          token,
        });

        if (cancelled) {
          return;
        }

        setRecords(response.data);
        setTotalCount(Number(response.headers.get("x-total-count") ?? 0));
      } catch (error) {
        if (!cancelled) {
          toast.error(
            error instanceof Error ? error.message : t.auditHistory.loadFailed,
          );
          setRecords([]);
          setTotalCount(0);
        }
      } finally {
        if (!cancelled) {
          setIsLoading(false);
        }
      }
    };

    void loadRecords();

    return () => {
      cancelled = true;
    };
  }, [queryPath, refreshKey, t.auditHistory.loadFailed, token]);

  useEffect(() => {
    if (page > pageCount) {
      setPage(pageCount);
    }
  }, [page, pageCount]);

  useEffect(() => {
    if (!selectedId) {
      return;
    }

    let cancelled = false;

    const loadDetail = async () => {
      setIsDetailLoading(true);

      try {
        const record = await apiRequest<SqlAuditRecord>(
          `/api/v1/sql-audits/${selectedId}`,
          { token },
        );

        if (!cancelled) {
          setSelectedRecord(record);
        }
      } catch (error) {
        if (!cancelled) {
          toast.error(
            error instanceof Error ? error.message : t.auditHistory.loadFailed,
          );
          setSelectedId(null);
          setSelectedRecord(null);
        }
      } finally {
        if (!cancelled) {
          setIsDetailLoading(false);
        }
      }
    };

    void loadDetail();

    return () => {
      cancelled = true;
    };
  }, [selectedId, t.auditHistory.loadFailed, token]);

  const resetFilters = () => {
    setDatabaseId("all");
    setAuditStatus("all");
    setExecutionStatus("all");
    setCreatedFrom("");
    setCreatedTo("");
    setPage(1);
  };

  const handleFilterChange = (updater: () => void) => {
    updater();
    setPage(1);
  };

  return (
    <>
      <Card className="min-h-[420px] flex-1 rounded-lg py-4 shadow-xs">
        <CardHeader className="flex flex-col gap-3 px-4 sm:flex-row sm:items-center sm:justify-between">
          <div className="min-w-0">
            <div className="flex items-center gap-2">
              <span className="flex size-8 shrink-0 items-center justify-center rounded-md bg-primary text-primary-foreground shadow-xs">
                <ClipboardCheck className="size-4" aria-hidden />
              </span>
              <div className="min-w-0">
                <CardTitle className="truncate text-sm">
                  {t.auditHistory.title}
                </CardTitle>
                <p className="mt-1 text-xs text-muted-foreground">
                  {t.auditHistory.totalCount(totalCount)}
                </p>
              </div>
            </div>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={resetFilters}
            >
              <RotateCcw className="size-4" aria-hidden />
              {t.auditHistory.resetFilters}
            </Button>
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={isLoading}
              onClick={() => setRefreshKey((current) => current + 1)}
            >
              {isLoading ? (
                <Loader2 className="size-4 animate-spin" aria-hidden />
              ) : (
                <RefreshCw className="size-4" aria-hidden />
              )}
              {t.auditHistory.refresh}
            </Button>
          </div>
        </CardHeader>
        <CardContent className="space-y-3 px-4">
          <div className="grid gap-2 rounded-lg border bg-muted/20 p-3 md:grid-cols-2 xl:grid-cols-5">
            <FilterField label={t.auditHistory.filters.database}>
              <FilterSelect
                value={databaseId}
                ariaLabel={t.auditHistory.filters.database}
                options={[
                  { value: "all", label: t.auditHistory.allDatabases },
                  ...databases.map((database) => ({
                    value: database.id,
                    label: database.name,
                  })),
                ]}
                onValueChange={(value) =>
                  handleFilterChange(() => setDatabaseId(value))
                }
              />
            </FilterField>
            <FilterField label={t.auditHistory.filters.createdFrom}>
              <DatePicker
                value={createdFrom}
                locale={locale}
                ariaLabel={t.auditHistory.filters.createdFrom}
                onValueChange={(value) =>
                  handleFilterChange(() => setCreatedFrom(value))
                }
              />
            </FilterField>
            <FilterField label={t.auditHistory.filters.createdTo}>
              <DatePicker
                value={createdTo}
                locale={locale}
                ariaLabel={t.auditHistory.filters.createdTo}
                onValueChange={(value) =>
                  handleFilterChange(() => setCreatedTo(value))
                }
              />
            </FilterField>
            <FilterField label={t.auditHistory.filters.auditStatus}>
              <FilterSelect
                value={auditStatus}
                ariaLabel={t.auditHistory.filters.auditStatus}
                options={[
                  { value: "all", label: t.auditHistory.allStatuses },
                  ...auditStatusOptions.map((status) => ({
                    value: status,
                    label: t.auditHistory.auditStatuses[status],
                  })),
                ]}
                onValueChange={(value) =>
                  handleFilterChange(() =>
                    setAuditStatus(value as FilterValue<SqlAuditLifecycleStatus>),
                  )
                }
              />
            </FilterField>
            <FilterField label={t.auditHistory.filters.executionStatus}>
              <FilterSelect
                value={executionStatus}
                ariaLabel={t.auditHistory.filters.executionStatus}
                options={[
                  {
                    value: "all",
                    label: t.auditHistory.allExecutionStatuses,
                  },
                  ...executionStatusOptions.map((status) => ({
                    value: status,
                    label: t.auditHistory.executionStatuses[status],
                  })),
                ]}
                onValueChange={(value) =>
                  handleFilterChange(() =>
                    setExecutionStatus(
                      value as FilterValue<SqlAuditExecutionStatus>,
                    ),
                  )
                }
              />
            </FilterField>
          </div>

          <div className="overflow-hidden rounded-lg border bg-background">
            {isLoading ? (
              <div className="flex items-center gap-2 p-4 text-sm text-muted-foreground">
                <Loader2 className="size-4 animate-spin" aria-hidden />
                {t.auditHistory.loading}
              </div>
            ) : records.length === 0 ? (
              <div className="flex min-h-48 flex-col items-center justify-center p-6 text-center">
                <div className="flex size-10 items-center justify-center rounded-md border bg-muted/40">
                  <FileText className="size-5 text-muted-foreground" aria-hidden />
                </div>
                <div className="mt-3 text-sm font-medium">
                  {t.auditHistory.emptyTitle}
                </div>
                <p className="mt-1 max-w-sm text-xs leading-5 text-muted-foreground">
                  {t.auditHistory.emptyDescription}
                </p>
              </div>
            ) : (
              <>
                <div className="hidden overflow-x-auto md:block">
                  <table className="w-full min-w-[920px] text-left text-sm">
                    <thead className="border-b bg-muted/40 text-xs text-muted-foreground">
                      <tr>
                        <th className="px-3 py-2 font-medium">
                          {t.auditHistory.table.id}
                        </th>
                        <th className="px-3 py-2 font-medium">
                          {t.auditHistory.table.database}
                        </th>
                        <th className="px-3 py-2 font-medium">
                          {t.auditHistory.table.sql}
                        </th>
                        <th className="px-3 py-2 font-medium">
                          {t.auditHistory.table.status}
                        </th>
                        <th className="px-3 py-2 font-medium">
                          {t.auditHistory.table.executionStatus}
                        </th>
                        <th className="px-3 py-2 font-medium">
                          {t.auditHistory.table.risk}
                        </th>
                        <th className="px-3 py-2 font-medium">
                          {t.auditHistory.table.statement}
                        </th>
                        <th className="px-3 py-2 font-medium">
                          {t.auditHistory.table.createdAt}
                        </th>
                      </tr>
                    </thead>
                    <tbody className="divide-y">
                      {records.map((record) => (
                        <AuditTableRow
                          key={record.id}
                          record={record}
                          locale={locale}
                          onOpen={() => {
                            setSelectedId(record.id);
                            setSelectedRecord(null);
                          }}
                        />
                      ))}
                    </tbody>
                  </table>
                </div>
                <div className="divide-y md:hidden">
                  {records.map((record) => (
                    <AuditMobileCard
                      key={record.id}
                      record={record}
                      locale={locale}
                      onOpen={() => {
                        setSelectedId(record.id);
                        setSelectedRecord(null);
                      }}
                    />
                  ))}
                </div>
              </>
            )}
          </div>

          <div className="flex flex-col gap-2 rounded-lg border bg-background p-3 text-sm sm:flex-row sm:items-center sm:justify-between">
            <div className="text-xs text-muted-foreground">
              {t.auditHistory.pageSummary(page, pageCount, totalCount)}
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <div className="flex items-center gap-2 text-xs text-muted-foreground">
                {t.auditHistory.pageSize}
                <FilterSelect
                  value={pageSize}
                  className="w-24"
                  ariaLabel={t.auditHistory.pageSize}
                  options={pageSizeOptions.map((size) => ({
                    value: size,
                    label: String(size),
                  }))}
                  onValueChange={(value) => {
                    setPageSize(Number(value) as PageSize);
                    setPage(1);
                  }}
                />
              </div>
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={page <= 1 || isLoading}
                onClick={() => setPage((current) => Math.max(1, current - 1))}
              >
                <ChevronLeft className="size-4" aria-hidden />
                {t.auditHistory.previousPage}
              </Button>
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={page >= pageCount || isLoading}
                onClick={() =>
                  setPage((current) => Math.min(pageCount, current + 1))
                }
              >
                {t.auditHistory.nextPage}
                <ChevronRight className="size-4" aria-hidden />
              </Button>
            </div>
          </div>
        </CardContent>
      </Card>

      {selectedId ? (
        <AuditDetailDialog
          record={selectedRecord}
          isLoading={isDetailLoading}
          locale={locale}
          onClose={() => {
            setSelectedId(null);
            setSelectedRecord(null);
          }}
        />
      ) : null}
    </>
  );
}

function FilterField({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="space-y-1.5">
      <span className="text-xs font-medium text-muted-foreground">{label}</span>
      {children}
    </div>
  );
}

type FilterSelectOption<T extends string | number> = {
  value: T;
  label: string;
};

function FilterSelect<T extends string | number>({
  value,
  options,
  ariaLabel,
  className,
  onValueChange,
}: {
  value: T;
  options: FilterSelectOption<T>[];
  ariaLabel: string;
  className?: string;
  onValueChange: (value: T) => void;
}) {
  const [isOpen, setIsOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const selected = options.find((option) => option.value === value);

  useEffect(() => {
    if (!isOpen) {
      return;
    }

    const handlePointerDown = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) {
        setIsOpen(false);
      }
    };

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setIsOpen(false);
      }
    };

    document.addEventListener("pointerdown", handlePointerDown);
    document.addEventListener("keydown", handleKeyDown);

    return () => {
      document.removeEventListener("pointerdown", handlePointerDown);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [isOpen]);

  return (
    <div ref={rootRef} className={cn("relative", className)}>
      <button
        type="button"
        className={cn(
          "flex h-9 w-full items-center justify-between gap-2 rounded-md border bg-background px-3 text-left text-sm outline-none transition-all hover:bg-accent/50 focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50",
          isOpen && "border-ring ring-[3px] ring-ring/50",
        )}
        aria-label={ariaLabel}
        aria-haspopup="listbox"
        aria-expanded={isOpen}
        onClick={() => setIsOpen((current) => !current)}
      >
        <span className="min-w-0 truncate text-foreground">
          {selected?.label ?? ""}
        </span>
        <ChevronDown
          className={cn(
            "size-4 shrink-0 text-muted-foreground transition-transform",
            isOpen && "rotate-180",
          )}
          aria-hidden
        />
      </button>

      {isOpen ? (
        <div
          className="absolute left-0 top-[calc(100%+0.25rem)] z-40 max-h-72 w-full min-w-44 overflow-y-auto rounded-md border bg-popover p-1 text-popover-foreground shadow-lg"
          role="listbox"
          aria-label={ariaLabel}
        >
          {options.map((option) => {
            const isSelected = option.value === value;

            return (
              <button
                key={String(option.value)}
                type="button"
                className={cn(
                  "flex h-8 w-full items-center rounded-sm px-2.5 text-left text-sm outline-none transition-colors hover:bg-accent hover:text-accent-foreground focus-visible:bg-accent focus-visible:text-accent-foreground",
                  isSelected && "bg-accent text-accent-foreground",
                )}
                role="option"
                aria-selected={isSelected}
                onClick={() => {
                  onValueChange(option.value);
                  setIsOpen(false);
                }}
              >
                <span className="min-w-0 truncate">{option.label}</span>
              </button>
            );
          })}
        </div>
      ) : null}
    </div>
  );
}

function DatePicker({
  value,
  locale,
  ariaLabel,
  onValueChange,
}: {
  value: string;
  locale: string;
  ariaLabel: string;
  onValueChange: (value: string) => void;
}) {
  const [isOpen, setIsOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const selectedDate = useMemo(() => parseDateValue(value), [value]);
  const [visibleMonth, setVisibleMonth] = useState(() =>
    startOfMonth(selectedDate ?? new Date()),
  );
  const today = startOfDay(new Date());
  const days = useMemo(() => monthGrid(visibleMonth), [visibleMonth]);
  const weekDays = useMemo(() => weekdayLabels(locale), [locale]);
  const clearLabel = locale === "zh-CN" ? "清除" : "Clear";
  const todayLabel = locale === "zh-CN" ? "今天" : "Today";
  const buttonLabel = selectedDate
    ? formatDateLabel(selectedDate, locale)
    : locale === "zh-CN"
      ? "年 /月 /日"
      : "yyyy / mm / dd";

  useEffect(() => {
    if (selectedDate) {
      setVisibleMonth(startOfMonth(selectedDate));
    }
  }, [selectedDate]);

  useEffect(() => {
    if (!isOpen) {
      return;
    }

    const handlePointerDown = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) {
        setIsOpen(false);
      }
    };

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setIsOpen(false);
      }
    };

    document.addEventListener("pointerdown", handlePointerDown);
    document.addEventListener("keydown", handleKeyDown);

    return () => {
      document.removeEventListener("pointerdown", handlePointerDown);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [isOpen]);

  return (
    <div ref={rootRef} className="relative">
      <button
        type="button"
        className={cn(
          "flex h-9 w-full items-center justify-between gap-2 rounded-md border bg-background px-3 text-left text-sm outline-none transition-all hover:bg-accent/50 focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50",
          isOpen && "border-ring ring-[3px] ring-ring/50",
        )}
        aria-label={ariaLabel}
        aria-haspopup="dialog"
        aria-expanded={isOpen}
        onClick={() => setIsOpen((current) => !current)}
      >
        <span
          className={cn(
            "min-w-0 truncate",
            selectedDate ? "text-foreground" : "text-muted-foreground",
          )}
        >
          {buttonLabel}
        </span>
        <CalendarDays className="size-4 shrink-0 text-muted-foreground" aria-hidden />
      </button>

      {isOpen ? (
        <div
          className="absolute left-0 top-[calc(100%+0.25rem)] z-40 w-[19rem] rounded-md border bg-popover p-3 text-popover-foreground shadow-lg"
          role="dialog"
          aria-label={ariaLabel}
        >
          <div className="flex items-center justify-between gap-2">
            <div className="truncate text-sm font-semibold">
              {formatMonthLabel(visibleMonth, locale)}
            </div>
            <div className="flex items-center gap-1">
              <Button
                type="button"
                variant="ghost"
                size="icon"
                className="size-8"
                aria-label="Previous month"
                onClick={() =>
                  setVisibleMonth((current) => addMonths(current, -1))
                }
              >
                <ChevronLeft className="size-4" aria-hidden />
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                className="size-8"
                aria-label="Next month"
                onClick={() =>
                  setVisibleMonth((current) => addMonths(current, 1))
                }
              >
                <ChevronRight className="size-4" aria-hidden />
              </Button>
            </div>
          </div>

          <div className="mt-3 grid grid-cols-7 gap-1 text-center text-xs font-medium text-muted-foreground">
            {weekDays.map((day) => (
              <div key={day} className="py-1">
                {day}
              </div>
            ))}
          </div>

          <div className="mt-1 grid grid-cols-7 gap-1">
            {days.map((day) => {
              const isSelected = selectedDate
                ? sameCalendarDate(day, selectedDate)
                : false;
              const isToday = sameCalendarDate(day, today);
              const isCurrentMonth =
                day.getMonth() === visibleMonth.getMonth() &&
                day.getFullYear() === visibleMonth.getFullYear();

              return (
                <button
                  key={formatDateValue(day)}
                  type="button"
                  className={cn(
                    "flex size-9 items-center justify-center rounded-md text-sm outline-none transition-colors hover:bg-accent hover:text-accent-foreground focus-visible:ring-[3px] focus-visible:ring-ring/50",
                    !isCurrentMonth && "text-muted-foreground/60",
                    isToday && !isSelected && "border border-border",
                    isSelected &&
                      "bg-primary font-semibold text-primary-foreground hover:bg-primary hover:text-primary-foreground",
                  )}
                  aria-pressed={isSelected}
                  onClick={() => {
                    onValueChange(formatDateValue(day));
                    setIsOpen(false);
                  }}
                >
                  {day.getDate()}
                </button>
              );
            })}
          </div>

          <div className="mt-3 flex items-center justify-between">
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="px-2 text-primary hover:text-primary"
              onClick={() => {
                onValueChange("");
                setIsOpen(false);
              }}
            >
              {clearLabel}
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="px-2 text-primary hover:text-primary"
              onClick={() => {
                onValueChange(formatDateValue(today));
                setVisibleMonth(startOfMonth(today));
                setIsOpen(false);
              }}
            >
              {todayLabel}
            </Button>
          </div>
        </div>
      ) : null}
    </div>
  );
}

function AuditTableRow({
  record,
  locale,
  onOpen,
}: {
  record: SqlAuditRecord;
  locale: string;
  onOpen: () => void;
}) {
  const { t } = useI18n();
  const executionStatus = deriveExecutionStatus(record.status);

  return (
    <tr className="transition-colors hover:bg-muted/30">
      <td className="px-3 py-2 align-top">
        <button
          type="button"
          className="max-w-32 truncate rounded-sm font-mono text-xs font-medium text-foreground underline-offset-4 outline-none hover:underline focus-visible:ring-[3px] focus-visible:ring-ring/50"
          title={record.id}
          onClick={onOpen}
        >
          {shortId(record.id)}
        </button>
      </td>
      <td className="px-3 py-2 align-top">
        <div className="max-w-36 truncate font-medium">
          {record.managed_database_name}
        </div>
        <div className="mt-1 max-w-44 truncate text-xs text-muted-foreground">
          {record.managed_database_host}/{record.managed_database_database}
        </div>
      </td>
      <td className="px-3 py-2 align-top">
        <div className="max-w-[260px] truncate font-mono text-xs">
          {record.sql}
        </div>
      </td>
      <td className="px-3 py-2 align-top">
        <StatusBadge status={record.status} />
      </td>
      <td className="px-3 py-2 align-top">
        <ExecutionStatusBadge status={executionStatus} />
      </td>
      <td className="px-3 py-2 align-top">
        <RiskBadge score={record.risk_score} />
      </td>
      <td className="px-3 py-2 align-top text-xs text-muted-foreground">
        {record.statement_kind ?? t.auditHistory.statementUnknown}
      </td>
      <td className="px-3 py-2 align-top text-xs text-muted-foreground">
        {formatDateTime(record.created_at, locale)}
      </td>
    </tr>
  );
}

function AuditMobileCard({
  record,
  locale,
  onOpen,
}: {
  record: SqlAuditRecord;
  locale: string;
  onOpen: () => void;
}) {
  const { t } = useI18n();
  const executionStatus = deriveExecutionStatus(record.status);

  return (
    <article className="p-3">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <button
            type="button"
            className="max-w-full truncate rounded-sm font-mono text-xs font-medium underline-offset-4 outline-none hover:underline focus-visible:ring-[3px] focus-visible:ring-ring/50"
            title={record.id}
            onClick={onOpen}
          >
            {shortId(record.id)}
          </button>
          <div className="mt-1 truncate text-sm font-medium">
            {record.managed_database_name}
          </div>
          <div className="mt-1 truncate font-mono text-xs text-muted-foreground">
            {record.sql}
          </div>
        </div>
        <RiskBadge score={record.risk_score} />
      </div>
      <div className="mt-3 flex flex-wrap gap-1.5">
        <StatusBadge status={record.status} />
        <ExecutionStatusBadge status={executionStatus} />
        <Badge variant="outline" className="rounded-md">
          {record.statement_kind ?? t.auditHistory.statementUnknown}
        </Badge>
      </div>
      <div className="mt-2 flex items-center gap-1.5 text-xs text-muted-foreground">
        <CalendarDays className="size-3.5" aria-hidden />
        {formatDateTime(record.created_at, locale)}
      </div>
    </article>
  );
}

function AuditDetailDialog({
  record,
  isLoading,
  locale,
  onClose,
}: {
  record: SqlAuditRecord | null;
  isLoading: boolean;
  locale: string;
  onClose: () => void;
}) {
  const { t } = useI18n();

  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center overflow-y-auto bg-background/80 p-3 sm:items-center">
      <button
        type="button"
        className="absolute inset-0 cursor-default"
        aria-label={t.auditHistory.closeDetail}
        onClick={onClose}
      />
      <Card
        className="relative w-full max-w-5xl overflow-hidden rounded-xl py-0 shadow-xl"
        role="dialog"
        aria-modal="true"
        aria-labelledby="sql-audit-detail-title"
      >
        <CardHeader className="flex flex-row items-start justify-between gap-4 border-b bg-muted/30 px-5 py-4">
          <div className="flex min-w-0 items-start gap-3">
            <span className="mt-0.5 flex size-10 shrink-0 items-center justify-center rounded-lg bg-primary text-primary-foreground shadow-sm">
              <ClipboardCheck className="size-5" aria-hidden />
            </span>
            <div className="min-w-0">
              <CardTitle id="sql-audit-detail-title" className="text-base">
                {t.auditHistory.detailTitle}
              </CardTitle>
              <p className="mt-1 truncate font-mono text-xs text-muted-foreground">
                {record?.id ?? t.auditHistory.loadingDetail}
              </p>
            </div>
          </div>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            aria-label={t.auditHistory.closeDetail}
            title={t.auditHistory.closeDetail}
            onClick={onClose}
          >
            <X className="size-4" aria-hidden />
          </Button>
        </CardHeader>
        <CardContent className="max-h-[calc(100vh-8rem)] overflow-x-hidden overflow-y-auto px-5 py-4">
          {isLoading || !record ? (
            <div className="flex items-center gap-2 rounded-lg border bg-background p-4 text-sm text-muted-foreground">
              <Loader2 className="size-4 animate-spin" aria-hidden />
              {t.auditHistory.loadingDetail}
            </div>
          ) : (
            <div className="grid min-w-0 gap-4 lg:grid-cols-[minmax(0,1.25fr)_minmax(320px,0.75fr)]">
              <div className="min-w-0 space-y-4">
                <DetailBlock title={t.auditHistory.sql}>
                  <pre className="max-h-80 max-w-full overflow-auto rounded-md border bg-muted/50 p-3 text-xs leading-5">
                    <code className="font-mono">{record.sql}</code>
                  </pre>
                </DetailBlock>
                <DetailBlock title={t.auditHistory.reportSummary}>
                  <p className="break-words text-sm leading-6 text-muted-foreground">
                    {record.report?.summary ?? t.auditHistory.noValue}
                  </p>
                </DetailBlock>
                <DetailBlock title={t.auditHistory.findings}>
                  {record.report?.findings.length ? (
                    <div className="space-y-2">
                      {record.report.findings.map((finding, index) => (
                        <article
                          key={`${finding.title}-${index}`}
                          className="rounded-lg border bg-background p-3"
                        >
                          <div className="flex flex-wrap items-center justify-between gap-2">
                            <h3 className="text-sm font-medium">
                              {finding.title}
                            </h3>
                            <SeverityBadge severity={finding.severity} />
                          </div>
                          <p className="mt-2 break-words text-sm leading-6 text-muted-foreground">
                            {finding.explanation}
                          </p>
                          <div className="mt-2 break-words rounded-md bg-muted/40 px-3 py-2 text-xs leading-5 text-muted-foreground">
                            <span className="font-medium text-foreground">
                              {t.auditHistory.findingRecommendation}
                            </span>{" "}
                            {finding.recommendation}
                          </div>
                        </article>
                      ))}
                    </div>
                  ) : (
                    <p className="text-sm text-muted-foreground">
                      {t.auditHistory.noFindings}
                    </p>
                  )}
                </DetailBlock>
              </div>
              <div className="min-w-0 space-y-4">
                <DetailBlock title={t.auditHistory.metadata}>
                  <div className="space-y-2">
                    <DetailRow label={t.auditHistory.table.status}>
                      <StatusBadge status={record.status} />
                    </DetailRow>
                    <DetailRow label={t.auditHistory.table.executionStatus}>
                      <ExecutionStatusBadge
                        status={deriveExecutionStatus(record.status)}
                      />
                    </DetailRow>
                    <DetailRow label={t.auditHistory.table.risk}>
                      <RiskBadge score={record.risk_score} />
                    </DetailRow>
                    <DetailRow label={t.auditHistory.table.statement}>
                      {record.statement_kind ?? t.auditHistory.statementUnknown}
                    </DetailRow>
                    <DetailRow label={t.auditHistory.createdAt}>
                      {formatDateTime(record.created_at, locale)}
                    </DetailRow>
                    <DetailRow label={t.auditHistory.updatedAt}>
                      {formatDateTime(record.updated_at, locale)}
                    </DetailRow>
                    {record.executed_at ? (
                      <DetailRow label={t.auditHistory.executedAt}>
                        {formatDateTime(record.executed_at, locale)}
                      </DetailRow>
                    ) : null}
                    {record.approved_at ? (
                      <DetailRow label={t.auditHistory.approvedAt}>
                        {formatDateTime(record.approved_at, locale)}
                      </DetailRow>
                    ) : null}
                    {record.rejected_at ? (
                      <DetailRow label={t.auditHistory.rejectedAt}>
                        {formatDateTime(record.rejected_at, locale)}
                      </DetailRow>
                    ) : null}
                  </div>
                </DetailBlock>
                <DetailBlock title={t.auditHistory.databaseSnapshot}>
                  <div className="space-y-2 text-sm">
                    <DetailRow label={t.auditHistory.table.database}>
                      {record.managed_database_name}
                    </DetailRow>
                    <DetailRow label="Host">
                      {record.managed_database_host}:{record.managed_database_port}
                    </DetailRow>
                    <DetailRow label="Database">
                      {record.managed_database_database}
                    </DetailRow>
                    <DetailRow label="User">
                      {record.managed_database_username}
                    </DetailRow>
                    <DetailRow label="SSL">
                      {record.managed_database_ssl_mode}
                    </DetailRow>
                  </div>
                </DetailBlock>
                <DetailBlock title={t.auditHistory.execution}>
                  <div className="space-y-2">
                    {record.execution_error ? (
                      <div className="flex gap-2 rounded-md border border-destructive/30 bg-destructive/10 p-3 text-sm text-destructive">
                        <AlertCircle className="mt-0.5 size-4 shrink-0" aria-hidden />
                        <span>{record.execution_error}</span>
                      </div>
                    ) : null}
                    {record.execution_result ? (
                      <>
                        <RollbackPlanDetail
                          rollback={record.execution_result.rollback}
                          locale={locale}
                        />
                        <pre className="max-h-52 max-w-full overflow-auto rounded-md border bg-muted/50 p-3 text-xs leading-5">
                          <code className="font-mono">
                            {JSON.stringify(record.execution_result, null, 2)}
                          </code>
                        </pre>
                      </>
                    ) : !record.execution_error ? (
                      <p className="text-sm text-muted-foreground">
                        {t.auditHistory.noValue}
                      </p>
                    ) : null}
                  </div>
                </DetailBlock>
              </div>
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}

function RollbackPlanDetail({
  rollback,
  locale,
}: {
  rollback?: SqlRollbackPlan | null;
  locale: string;
}) {
  const { t } = useI18n();

  if (!rollback) {
    return null;
  }

  const copySql = async () => {
    if (!rollback.sql) {
      return;
    }

    try {
      await navigator.clipboard.writeText(rollback.sql);
      toast.success(t.auditHistory.rollbackCopied);
    } catch (error) {
      toast.error(error instanceof Error ? error.message : t.workspace.copyFailed);
    }
  };

  return (
    <div className="space-y-2 rounded-md border bg-background p-3">
      <div className="flex flex-wrap items-center gap-2">
        <RotateCcw className="size-4 text-muted-foreground" aria-hidden />
        <span className="text-xs font-medium uppercase text-muted-foreground">
          {t.auditHistory.rollbackTitle}
        </span>
        <Badge variant="outline" className="font-mono">
          {t.auditHistory.rollbackStatuses[rollback.status]}
        </Badge>
      </div>
      {rollback.reason ? (
        <p className="break-words text-xs leading-5 text-muted-foreground">
          <span className="font-medium text-foreground">
            {t.auditHistory.rollbackReason}
          </span>{" "}
          {rollback.reason}
        </p>
      ) : null}
      {rollback.generated_at ? (
        <p className="text-xs text-muted-foreground">
          <span className="font-medium text-foreground">
            {t.auditHistory.rollbackGeneratedAt}
          </span>{" "}
          {formatDateTime(rollback.generated_at, locale)}
        </p>
      ) : null}
      {rollback.sql ? (
        <div className="overflow-hidden rounded-md border bg-muted/50">
          <div className="flex h-8 items-center justify-between border-b bg-muted px-3">
            <span className="text-[11px] font-medium uppercase text-muted-foreground">
              {t.auditHistory.rollbackSql}
            </span>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="h-7 rounded-md px-2 text-xs"
              onClick={() => void copySql()}
            >
              <Copy className="size-3.5" aria-hidden />
              {t.workspace.copy}
            </Button>
          </div>
          <pre className="max-h-52 overflow-auto p-3 text-xs leading-5">
            <code className="font-mono">{rollback.sql}</code>
          </pre>
        </div>
      ) : null}
    </div>
  );
}

function DetailBlock({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <section className="min-w-0 rounded-lg border bg-background p-3">
      <h2 className="text-sm font-semibold">{title}</h2>
      <div className="mt-3">{children}</div>
    </section>
  );
}

function DetailRow({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1 border-b py-2 last:border-b-0 sm:flex-row sm:items-start sm:justify-between sm:gap-3">
      <span className="text-xs text-muted-foreground">{label}</span>
      <span className="min-w-0 break-words text-sm sm:text-right">{children}</span>
    </div>
  );
}

function StatusBadge({ status }: { status: SqlAuditStatus }) {
  const { t } = useI18n();

  return (
    <Badge
      variant={status === "blocked" || status === "rejected" ? "destructive" : "secondary"}
      className="rounded-md"
    >
      {t.auditHistory.statuses[status]}
    </Badge>
  );
}

function ExecutionStatusBadge({ status }: { status: SqlAuditExecutionStatus }) {
  const { t } = useI18n();

  return (
    <Badge
      variant={status === "execution_failed" ? "destructive" : "outline"}
      className={cn(
        "rounded-md",
        status === "executed" && "border-emerald-200 bg-emerald-50 text-emerald-800",
      )}
    >
      {t.auditHistory.executionStatuses[status]}
    </Badge>
  );
}

function RiskBadge({ score }: { score: number }) {
  return (
    <Badge
      variant="outline"
      className={cn(
        "rounded-md font-semibold tabular-nums",
        severityBadgeClass(riskScoreSeverity(score)),
      )}
    >
      {score}
    </Badge>
  );
}

function SeverityBadge({ severity }: { severity: RiskSeverity }) {
  const { t } = useI18n();

  return (
    <Badge
      variant="outline"
      className={cn("rounded-md font-semibold", severityBadgeClass(severity))}
    >
      {t.auditHistory.severities[severity]}
    </Badge>
  );
}

function severityBadgeClass(severity: RiskSeverity) {
  switch (severity) {
    case "low":
      return "border-emerald-500/35 bg-emerald-500/15 text-emerald-700 dark:text-emerald-300";
    case "medium":
      return "border-amber-500/45 bg-amber-400/20 text-amber-800 dark:text-amber-300";
    case "high":
      return "border-orange-500/45 bg-orange-500/20 text-orange-800 dark:text-orange-300";
    case "critical":
      return "border-red-500/45 bg-red-500/20 text-red-700 dark:text-red-300";
  }
}

function riskScoreSeverity(score: number): RiskSeverity {
  if (score >= 90) {
    return "critical";
  }

  if (score >= 75) {
    return "high";
  }

  if (score >= 50) {
    return "medium";
  }

  return "low";
}

const auditStatusOptions: SqlAuditLifecycleStatus[] = [
  "audited",
  "pending_approval",
  "approved",
  "rejected",
  "blocked",
];

const executionStatusOptions: SqlAuditExecutionStatus[] = [
  "not_executed",
  "executing",
  "executed",
  "execution_failed",
];

function deriveExecutionStatus(status: SqlAuditStatus): SqlAuditExecutionStatus {
  if (
    status === "executing" ||
    status === "executed" ||
    status === "execution_failed"
  ) {
    return status;
  }

  return "not_executed";
}

function parseDateValue(value: string) {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);

  if (!match) {
    return null;
  }

  const year = Number(match[1]);
  const month = Number(match[2]) - 1;
  const day = Number(match[3]);
  const date = new Date(year, month, day);

  if (
    date.getFullYear() !== year ||
    date.getMonth() !== month ||
    date.getDate() !== day
  ) {
    return null;
  }

  return date;
}

function formatDateValue(date: Date) {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");

  return `${year}-${month}-${day}`;
}

function formatDateLabel(date: Date, locale: string) {
  return new Intl.DateTimeFormat(locale, {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  }).format(date);
}

function formatMonthLabel(date: Date, locale: string) {
  return new Intl.DateTimeFormat(locale, {
    year: "numeric",
    month: "long",
  }).format(date);
}

function startOfDay(date: Date) {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate());
}

function startOfMonth(date: Date) {
  return new Date(date.getFullYear(), date.getMonth(), 1);
}

function addMonths(date: Date, amount: number) {
  return new Date(date.getFullYear(), date.getMonth() + amount, 1);
}

function monthGrid(month: Date) {
  const firstDay = startOfMonth(month);
  const start = new Date(firstDay);

  start.setDate(firstDay.getDate() - firstDay.getDay());

  return Array.from({ length: 42 }, (_, index) => {
    const date = new Date(start);
    date.setDate(start.getDate() + index);
    return date;
  });
}

function weekdayLabels(locale: string) {
  const sunday = new Date(2026, 5, 7);

  return Array.from({ length: 7 }, (_, index) => {
    const date = new Date(sunday);
    date.setDate(sunday.getDate() + index);

    return new Intl.DateTimeFormat(locale, { weekday: "narrow" }).format(date);
  });
}

function sameCalendarDate(left: Date, right: Date) {
  return (
    left.getFullYear() === right.getFullYear() &&
    left.getMonth() === right.getMonth() &&
    left.getDate() === right.getDate()
  );
}

function localDateBoundary(value: string, boundary: "start" | "end") {
  if (!value) {
    return null;
  }

  const date = new Date(`${value}T00:00:00`);

  if (Number.isNaN(date.getTime())) {
    return null;
  }

  if (boundary === "end") {
    date.setDate(date.getDate() + 1);
  }

  return date.toISOString();
}

function formatDateTime(value: string, locale: string) {
  return new Intl.DateTimeFormat(locale, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(value));
}

function shortId(id: string) {
  return id.length > 12 ? `${id.slice(0, 8)}...${id.slice(-4)}` : id;
}
