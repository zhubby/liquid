"use client";

import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertCircle,
  CalendarDays,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  Clock3,
  Database,
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
import { RecordIdButton } from "@/components/record-id-button";
import {
  type DatabaseBackupStatus,
  type DatabaseRestoreRecord,
  type ManagedDatabase,
  apiRequest,
  apiRequestWithMeta,
} from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

type DatabaseRestoreHistoryProps = {
  token: string;
  databases: ManagedDatabase[];
};

const pageSizeOptions = [10, 20, 50, 100] as const;
const restoreStatuses: DatabaseBackupStatus[] = [
  "queued",
  "running",
  "succeeded",
  "failed",
  "deleted",
];

type PageSize = (typeof pageSizeOptions)[number];
type FilterValue<T extends string> = "all" | T;

export function DatabaseRestoreHistory({
  token,
  databases,
}: DatabaseRestoreHistoryProps) {
  const { locale, t } = useI18n();
  const [records, setRecords] = useState<DatabaseRestoreRecord[]>([]);
  const [totalCount, setTotalCount] = useState(0);
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState<PageSize>(20);
  const [targetDatabaseId, setTargetDatabaseId] = useState("all");
  const [backupId, setBackupId] = useState("");
  const [status, setStatus] =
    useState<FilterValue<DatabaseBackupStatus>>("all");
  const [refreshKey, setRefreshKey] = useState(0);
  const [isLoading, setIsLoading] = useState(true);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [selectedRecord, setSelectedRecord] =
    useState<DatabaseRestoreRecord | null>(null);
  const [isDetailLoading, setIsDetailLoading] = useState(false);

  const pageCount = Math.max(1, Math.ceil(totalCount / pageSize));

  const queryPath = useMemo(() => {
    const params = new URLSearchParams({
      page: String(page),
      page_size: String(pageSize),
    });
    const backupIdFilter = backupId.trim();

    if (targetDatabaseId !== "all") {
      params.set("target_managed_database_id", targetDatabaseId);
    }

    if (backupIdFilter) {
      params.set("backup_id", backupIdFilter);
    }

    if (status !== "all") {
      params.set("status", status);
    }

    return `/api/v1/database-restores?${params.toString()}`;
  }, [backupId, page, pageSize, status, targetDatabaseId]);

  useEffect(() => {
    let cancelled = false;

    const loadRecords = async () => {
      setIsLoading(true);

      try {
        const response = await apiRequestWithMeta<DatabaseRestoreRecord[]>(
          queryPath,
          { token },
        );

        if (cancelled) {
          return;
        }

        setRecords(response.data);
        setTotalCount(
          Number(response.headers.get("x-total-count") ?? response.data.length),
        );
      } catch (error) {
        if (!cancelled) {
          toast.error(
            error instanceof Error ? error.message : t.restoreHistory.loadFailed,
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
  }, [queryPath, refreshKey, t.restoreHistory.loadFailed, token]);

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
        const record = await apiRequest<DatabaseRestoreRecord>(
          `/api/v1/database-restores/${selectedId}`,
          { token },
        );

        if (!cancelled) {
          setSelectedRecord(record);
        }
      } catch (error) {
        if (!cancelled) {
          toast.error(
            error instanceof Error ? error.message : t.restoreHistory.loadFailed,
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
  }, [selectedId, t.restoreHistory.loadFailed, token]);

  const resetFilters = () => {
    setTargetDatabaseId("all");
    setBackupId("");
    setStatus("all");
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
                <RotateCcw className="size-4" aria-hidden />
              </span>
              <div className="min-w-0">
                <CardTitle className="truncate text-sm">
                  {t.restoreHistory.title}
                </CardTitle>
                <p className="mt-1 text-xs text-muted-foreground">
                  {t.restoreHistory.totalCount(totalCount)}
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
              {t.restoreHistory.resetFilters}
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
              {t.restoreHistory.refresh}
            </Button>
          </div>
        </CardHeader>

        <CardContent className="space-y-3 px-4">
          <div className="grid gap-2 rounded-lg border bg-muted/20 p-3 md:grid-cols-3">
            <FilterField label={t.restoreHistory.filters.targetDatabase}>
              <FilterSelect
                value={targetDatabaseId}
                ariaLabel={t.restoreHistory.filters.targetDatabase}
                options={[
                  {
                    value: "all",
                    label: t.restoreHistory.allDatabases,
                  },
                  ...databases.map((database) => ({
                    value: database.id,
                    label: database.name,
                  })),
                ]}
                onValueChange={(value) =>
                  handleFilterChange(() => setTargetDatabaseId(String(value)))
                }
              />
            </FilterField>
            <FilterField label={t.restoreHistory.filters.status}>
              <FilterSelect
                value={status}
                ariaLabel={t.restoreHistory.filters.status}
                options={[
                  {
                    value: "all",
                    label: t.restoreHistory.allStatuses,
                  },
                  ...restoreStatuses.map((status) => ({
                    value: status,
                    label: t.restoreHistory.statuses[status],
                  })),
                ]}
                onValueChange={(value) =>
                  handleFilterChange(() =>
                    setStatus(value as FilterValue<DatabaseBackupStatus>),
                  )
                }
              />
            </FilterField>
            <FilterField label={t.restoreHistory.filters.backupId}>
              <input
                value={backupId}
                onChange={(event) =>
                  handleFilterChange(() => setBackupId(event.target.value))
                }
                placeholder={t.restoreHistory.backupIdPlaceholder}
                className="h-9 w-full rounded-md border bg-background px-3 text-sm outline-none transition-shadow placeholder:text-muted-foreground focus-visible:ring-[3px] focus-visible:ring-ring/50"
              />
            </FilterField>
          </div>

          <div className="overflow-hidden rounded-lg border bg-background">
            {isLoading ? (
              <div className="flex items-center gap-2 p-4 text-sm text-muted-foreground">
                <Loader2 className="size-4 animate-spin" aria-hidden />
                {t.restoreHistory.loading}
              </div>
            ) : records.length === 0 ? (
              <div className="flex min-h-48 flex-col items-center justify-center p-6 text-center">
                <div className="flex size-10 items-center justify-center rounded-md border bg-muted/40">
                  <RotateCcw
                    className="size-5 text-muted-foreground"
                    aria-hidden
                  />
                </div>
                <div className="mt-3 text-sm font-medium">
                  {t.restoreHistory.emptyTitle}
                </div>
                <p className="mt-1 max-w-sm text-xs leading-5 text-muted-foreground">
                  {t.restoreHistory.emptyDescription}
                </p>
              </div>
            ) : (
              <>
                <div className="hidden overflow-x-auto md:block">
                  <table className="w-full min-w-[1040px] text-left text-sm">
                    <thead className="border-b bg-muted/40 text-xs text-muted-foreground">
                      <tr>
                        <th className="px-3 py-2 font-medium">
                          {t.restoreHistory.table.id}
                        </th>
                        <th className="px-3 py-2 font-medium">
                          {t.restoreHistory.table.targetDatabase}
                        </th>
                        <th className="px-3 py-2 font-medium">
                          {t.restoreHistory.table.backup}
                        </th>
                        <th className="px-3 py-2 font-medium">
                          {t.restoreHistory.table.status}
                        </th>
                        <th className="px-3 py-2 font-medium">
                          {t.restoreHistory.table.phase}
                        </th>
                        <th className="px-3 py-2 font-medium">
                          {t.restoreHistory.table.purpose}
                        </th>
                        <th className="px-3 py-2 font-medium">
                          {t.restoreHistory.table.createdAt}
                        </th>
                        <th className="px-3 py-2 font-medium">
                          {t.restoreHistory.table.completedAt}
                        </th>
                      </tr>
                    </thead>
                    <tbody className="divide-y">
                      {records.map((record) => (
                        <RestoreTableRow
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
                    <RestoreMobileCard
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
              {t.restoreHistory.pageSummary(page, pageCount, totalCount)}
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <div className="flex items-center gap-2 text-xs text-muted-foreground">
                {t.restoreHistory.pageSize}
                <FilterSelect
                  value={pageSize}
                  className="w-24"
                  ariaLabel={t.restoreHistory.pageSize}
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
                {t.restoreHistory.previousPage}
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
                {t.restoreHistory.nextPage}
                <ChevronRight className="size-4" aria-hidden />
              </Button>
            </div>
          </div>
        </CardContent>
      </Card>

      {selectedId ? (
        <RestoreDetailDialog
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

function RestoreTableRow({
  record,
  locale,
  onOpen,
}: {
  record: DatabaseRestoreRecord;
  locale: string;
  onOpen: () => void;
}) {
  const { t } = useI18n();

  return (
    <tr className="transition-colors hover:bg-muted/30">
      <td className="px-3 py-2 align-top">
        <RecordIdButton
          id={record.id}
          label={shortId(record.id)}
          onOpen={onOpen}
        />
      </td>
      <td className="px-3 py-2 align-top">
        <div className="max-w-36 truncate font-medium">
          {record.target.name}
        </div>
        <div className="mt-1 max-w-44 truncate text-xs text-muted-foreground">
          {record.target.host}/{record.target.database}
        </div>
      </td>
      <td className="px-3 py-2 align-top">
        <span className="block max-w-32 truncate font-mono text-xs text-muted-foreground">
          {shortId(record.backup_id)}
        </span>
      </td>
      <td className="px-3 py-2 align-top">
        <RestoreStatusBadge status={record.status} />
      </td>
      <td className="px-3 py-2 align-top">
        <div className="text-xs text-muted-foreground">
          <span className="font-medium text-foreground">{record.phase}</span>
          <span className="ml-1">{record.progress_percent}%</span>
        </div>
      </td>
      <td className="px-3 py-2 align-top text-xs text-muted-foreground">
        <span className="block max-w-44 truncate">
          {record.purpose ?? t.restoreHistory.noValue}
        </span>
      </td>
      <td className="px-3 py-2 align-top text-xs text-muted-foreground">
        {formatDateTime(record.created_at, locale)}
      </td>
      <td className="px-3 py-2 align-top text-xs text-muted-foreground">
        {record.completed_at
          ? formatDateTime(record.completed_at, locale)
          : t.restoreHistory.noValue}
      </td>
    </tr>
  );
}

function RestoreMobileCard({
  record,
  locale,
  onOpen,
}: {
  record: DatabaseRestoreRecord;
  locale: string;
  onOpen: () => void;
}) {
  const { t } = useI18n();

  return (
    <article className="p-3">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <RecordIdButton
            id={record.id}
            label={shortId(record.id)}
            onOpen={onOpen}
            className="max-w-full"
          />
          <div className="mt-1 truncate text-sm font-medium">
            {record.target.name}
          </div>
          <div className="mt-1 truncate text-xs text-muted-foreground">
            {record.target.host}/{record.target.database}
          </div>
        </div>
        <RestoreStatusBadge status={record.status} />
      </div>
      <div className="mt-3 flex flex-wrap gap-1.5">
        <Badge variant="outline" className="rounded-md font-mono">
          {record.phase} · {record.progress_percent}%
        </Badge>
        <Badge variant="outline" className="rounded-md font-mono">
          {shortId(record.backup_id)}
        </Badge>
      </div>
      <div className="mt-2 flex items-center gap-1.5 text-xs text-muted-foreground">
        <CalendarDays className="size-3.5" aria-hidden />
        {formatDateTime(record.created_at, locale)}
      </div>
      {record.purpose ? (
        <div className="mt-1 truncate text-xs text-muted-foreground">
          {record.purpose}
        </div>
      ) : null}
      <span className="sr-only">{t.restoreHistory.table.backup}</span>
    </article>
  );
}

function RestoreDetailDialog({
  record,
  isLoading,
  locale,
  onClose,
}: {
  record: DatabaseRestoreRecord | null;
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
        aria-label={t.restoreHistory.closeDetail}
        onClick={onClose}
      />
      <Card
        className="relative w-full max-w-5xl overflow-hidden rounded-xl py-0 shadow-xl"
        role="dialog"
        aria-modal="true"
        aria-labelledby="database-restore-detail-title"
      >
        <CardHeader className="flex flex-row items-start justify-between gap-4 border-b bg-muted/30 px-5 py-4">
          <div className="flex min-w-0 items-start gap-3">
            <span className="mt-0.5 flex size-10 shrink-0 items-center justify-center rounded-lg bg-primary text-primary-foreground shadow-sm">
              <RotateCcw className="size-5" aria-hidden />
            </span>
            <div className="min-w-0">
              <CardTitle id="database-restore-detail-title" className="text-base">
                {t.restoreHistory.detailTitle}
              </CardTitle>
              <p className="mt-1 truncate font-mono text-xs text-muted-foreground">
                {record?.id ?? t.restoreHistory.loadingDetail}
              </p>
            </div>
          </div>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            aria-label={t.restoreHistory.closeDetail}
            title={t.restoreHistory.closeDetail}
            onClick={onClose}
          >
            <X className="size-4" aria-hidden />
          </Button>
        </CardHeader>
        <CardContent className="max-h-[calc(100vh-8rem)] overflow-x-hidden overflow-y-auto px-5 py-4">
          {isLoading || !record ? (
            <div className="flex items-center gap-2 rounded-lg border bg-background p-4 text-sm text-muted-foreground">
              <Loader2 className="size-4 animate-spin" aria-hidden />
              {t.restoreHistory.loadingDetail}
            </div>
          ) : (
            <div className="grid min-w-0 gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(320px,0.8fr)]">
              <div className="min-w-0 space-y-4">
                <DetailBlock title={t.restoreHistory.metadata}>
                  <div className="space-y-2">
                    <DetailRow label={t.restoreHistory.table.status}>
                      <RestoreStatusBadge status={record.status} />
                    </DetailRow>
                    <DetailRow label={t.restoreHistory.table.phase}>
                      <span className="font-mono">
                        {record.phase} · {record.progress_percent}%
                      </span>
                    </DetailRow>
                    <DetailRow label={t.restoreHistory.table.backup}>
                      <span className="font-mono">{record.backup_id}</span>
                    </DetailRow>
                    <DetailRow label={t.restoreHistory.purpose}>
                      {record.purpose ?? t.restoreHistory.noValue}
                    </DetailRow>
                    <DetailRow label={t.restoreHistory.worker}>
                      {record.worker_id ?? t.restoreHistory.noValue}
                    </DetailRow>
                    <DetailRow label={t.restoreHistory.format}>
                      {record.format}
                    </DetailRow>
                  </div>
                </DetailBlock>

                <DetailBlock title={t.restoreHistory.targetSnapshot}>
                  <div className="space-y-2 text-sm">
                    <DetailRow label={t.restoreHistory.table.targetDatabase}>
                      {record.target.name}
                    </DetailRow>
                    <DetailRow label="Host">
                      {record.target.host}:{record.target.port}
                    </DetailRow>
                    <DetailRow label="Database">
                      {record.target.database}
                    </DetailRow>
                    <DetailRow label="User">{record.target.username}</DetailRow>
                    <DetailRow label="SSL">{record.target.ssl_mode}</DetailRow>
                  </div>
                </DetailBlock>

                {record.error ? (
                  <DetailBlock title={t.restoreHistory.error}>
                    <div className="flex gap-2 rounded-md border border-destructive/30 bg-destructive/10 p-3 text-sm text-destructive">
                      <AlertCircle className="mt-0.5 size-4 shrink-0" aria-hidden />
                      <span className="break-words">{record.error}</span>
                    </div>
                  </DetailBlock>
                ) : null}
              </div>

              <div className="min-w-0 space-y-4">
                <DetailBlock title={t.restoreHistory.context}>
                  <div className="space-y-2 text-sm">
                    <DetailRow label={t.restoreHistory.conversationId}>
                      {record.conversation_id ?? t.restoreHistory.noValue}
                    </DetailRow>
                    <DetailRow label={t.restoreHistory.turnId}>
                      {record.created_from_turn_id ?? t.restoreHistory.noValue}
                    </DetailRow>
                  </div>
                </DetailBlock>

                <DetailBlock title={t.restoreHistory.timeline}>
                  <div className="space-y-2 text-sm">
                    <TimeRow
                      icon={<CalendarDays className="size-3.5" aria-hidden />}
                      label={t.restoreHistory.table.createdAt}
                      value={formatDateTime(record.created_at, locale)}
                    />
                    <TimeRow
                      icon={<RefreshCw className="size-3.5" aria-hidden />}
                      label={t.restoreHistory.updatedAt}
                      value={formatDateTime(record.updated_at, locale)}
                    />
                    <TimeRow
                      icon={<Clock3 className="size-3.5" aria-hidden />}
                      label={t.restoreHistory.startedAt}
                      value={
                        record.started_at
                          ? formatDateTime(record.started_at, locale)
                          : t.restoreHistory.noValue
                      }
                    />
                    <TimeRow
                      icon={<Database className="size-3.5" aria-hidden />}
                      label={t.restoreHistory.table.completedAt}
                      value={
                        record.completed_at
                          ? formatDateTime(record.completed_at, locale)
                          : t.restoreHistory.noValue
                      }
                    />
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

function RestoreStatusBadge({ status }: { status: DatabaseBackupStatus }) {
  const { t } = useI18n();
  const variant =
    status === "failed" || status === "deleted"
      ? "destructive"
      : status === "succeeded"
        ? "secondary"
        : "outline";

  return (
    <Badge variant={variant} className="rounded-md font-mono">
      {t.restoreHistory.statuses[status]}
    </Badge>
  );
}

function FilterField({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) {
  return (
    <div className="space-y-1.5">
      <span className="text-xs font-medium text-muted-foreground">{label}</span>
      {children}
    </div>
  );
}

function FilterSelect<T extends string | number>({
  value,
  options,
  ariaLabel,
  className,
  onValueChange,
}: {
  value: T;
  options: Array<{ value: T; label: string }>;
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
        aria-haspopup="listbox"
        aria-expanded={isOpen}
        aria-label={ariaLabel}
        onClick={() => setIsOpen((current) => !current)}
      >
        <span className="truncate">{selected?.label ?? String(value)}</span>
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
          className="absolute left-0 top-[calc(100%+0.25rem)] z-20 max-h-72 w-full overflow-y-auto rounded-md border bg-popover p-1 text-popover-foreground shadow-md"
          role="listbox"
        >
          {options.map((option) => (
            <button
              key={String(option.value)}
              type="button"
              className={cn(
                "flex w-full items-center rounded-sm px-2 py-1.5 text-left text-sm outline-none transition-colors hover:bg-accent hover:text-accent-foreground focus-visible:bg-accent focus-visible:text-accent-foreground",
                option.value === value && "bg-accent text-accent-foreground",
              )}
              role="option"
              aria-selected={option.value === value}
              onClick={() => {
                onValueChange(option.value);
                setIsOpen(false);
              }}
            >
              <span className="truncate">{option.label}</span>
            </button>
          ))}
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
  children: ReactNode;
}) {
  return (
    <section className="rounded-lg border bg-background p-4">
      <h3 className="text-sm font-semibold">{title}</h3>
      <div className="mt-3">{children}</div>
    </section>
  );
}

function DetailRow({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) {
  return (
    <div className="grid gap-1 text-sm sm:grid-cols-[8rem_minmax(0,1fr)]">
      <div className="text-xs font-medium text-muted-foreground">{label}</div>
      <div className="min-w-0 break-words text-foreground">{children}</div>
    </div>
  );
}

function TimeRow({
  icon,
  label,
  value,
}: {
  icon: ReactNode;
  label: string;
  value: string;
}) {
  return (
    <div className="flex items-center gap-2 rounded-md border bg-muted/20 px-3 py-2 text-xs">
      <span className="text-muted-foreground">{icon}</span>
      <span className="min-w-24 text-muted-foreground">{label}</span>
      <span className="ml-auto text-right font-medium">{value}</span>
    </div>
  );
}

function shortId(value: string) {
  return value.length > 12 ? `${value.slice(0, 8)}...${value.slice(-4)}` : value;
}

function formatDateTime(value: string, locale: string) {
  return new Intl.DateTimeFormat(locale, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(value));
}
