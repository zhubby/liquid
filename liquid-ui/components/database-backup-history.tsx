"use client";

import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertCircle,
  Archive,
  CalendarDays,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  Clock3,
  HardDrive,
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
  type DatabaseBackupRecord,
  type DatabaseBackupStatus,
  type DatabaseBackupTrigger,
  type ManagedDatabase,
  apiRequest,
  apiRequestWithMeta,
} from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

type DatabaseBackupHistoryProps = {
  token: string;
  databases: ManagedDatabase[];
};

const pageSizeOptions = [10, 20, 50, 100] as const;
const backupStatuses: DatabaseBackupStatus[] = [
  "queued",
  "running",
  "succeeded",
  "failed",
  "deleted",
];
const backupTriggers: DatabaseBackupTrigger[] = ["immediate", "cron"];

type PageSize = (typeof pageSizeOptions)[number];
type FilterValue<T extends string> = "all" | T;

export function DatabaseBackupHistory({
  token,
  databases,
}: DatabaseBackupHistoryProps) {
  const { locale, t } = useI18n();
  const [records, setRecords] = useState<DatabaseBackupRecord[]>([]);
  const [totalCount, setTotalCount] = useState(0);
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState<PageSize>(20);
  const [databaseId, setDatabaseId] = useState("all");
  const [status, setStatus] =
    useState<FilterValue<DatabaseBackupStatus>>("all");
  const [trigger, setTrigger] =
    useState<FilterValue<DatabaseBackupTrigger>>("all");
  const [refreshKey, setRefreshKey] = useState(0);
  const [isLoading, setIsLoading] = useState(true);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [selectedRecord, setSelectedRecord] =
    useState<DatabaseBackupRecord | null>(null);
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

    if (status !== "all") {
      params.set("status", status);
    }

    if (trigger !== "all") {
      params.set("trigger", trigger);
    }

    return `/api/v1/database-backups?${params.toString()}`;
  }, [databaseId, page, pageSize, status, trigger]);

  useEffect(() => {
    let cancelled = false;

    const loadRecords = async () => {
      setIsLoading(true);

      try {
        const response = await apiRequestWithMeta<DatabaseBackupRecord[]>(
          queryPath,
          { token },
        );

        if (cancelled) {
          return;
        }

        setRecords(response.data);
        setTotalCount(Number(response.headers.get("x-total-count") ?? 0));
      } catch (error) {
        if (!cancelled) {
          toast.error(
            error instanceof Error ? error.message : t.backupHistory.loadFailed,
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
  }, [queryPath, refreshKey, t.backupHistory.loadFailed, token]);

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
        const record = await apiRequest<DatabaseBackupRecord>(
          `/api/v1/database-backups/${selectedId}`,
          { token },
        );

        if (!cancelled) {
          setSelectedRecord(record);
        }
      } catch (error) {
        if (!cancelled) {
          toast.error(
            error instanceof Error ? error.message : t.backupHistory.loadFailed,
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
  }, [selectedId, t.backupHistory.loadFailed, token]);

  const resetFilters = () => {
    setDatabaseId("all");
    setStatus("all");
    setTrigger("all");
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
                <Archive className="size-4" aria-hidden />
              </span>
              <div className="min-w-0">
                <CardTitle className="truncate text-sm">
                  {t.backupHistory.title}
                </CardTitle>
                <p className="mt-1 text-xs text-muted-foreground">
                  {t.backupHistory.totalCount(totalCount)}
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
              {t.backupHistory.resetFilters}
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
              {t.backupHistory.refresh}
            </Button>
          </div>
        </CardHeader>

        <CardContent className="space-y-3 px-4">
          <div className="grid gap-2 rounded-lg border bg-muted/20 p-3 md:grid-cols-3">
            <FilterField label={t.backupHistory.filters.database}>
              <FilterSelect
                value={databaseId}
                ariaLabel={t.backupHistory.filters.database}
                options={[
                  {
                    value: "all",
                    label: t.backupHistory.allDatabases,
                  },
                  ...databases.map((database) => ({
                    value: database.id,
                    label: database.name,
                  })),
                ]}
                onValueChange={(value) =>
                  handleFilterChange(() => setDatabaseId(String(value)))
                }
              />
            </FilterField>
            <FilterField label={t.backupHistory.filters.status}>
              <FilterSelect
                value={status}
                ariaLabel={t.backupHistory.filters.status}
                options={[
                  {
                    value: "all",
                    label: t.backupHistory.allStatuses,
                  },
                  ...backupStatuses.map((status) => ({
                    value: status,
                    label: t.backupHistory.statuses[status],
                  })),
                ]}
                onValueChange={(value) =>
                  handleFilterChange(() =>
                    setStatus(value as FilterValue<DatabaseBackupStatus>),
                  )
                }
              />
            </FilterField>
            <FilterField label={t.backupHistory.filters.trigger}>
              <FilterSelect
                value={trigger}
                ariaLabel={t.backupHistory.filters.trigger}
                options={[
                  {
                    value: "all",
                    label: t.backupHistory.allTriggers,
                  },
                  ...backupTriggers.map((trigger) => ({
                    value: trigger,
                    label: t.backupHistory.triggers[trigger],
                  })),
                ]}
                onValueChange={(value) =>
                  handleFilterChange(() =>
                    setTrigger(value as FilterValue<DatabaseBackupTrigger>),
                  )
                }
              />
            </FilterField>
          </div>

          <div className="overflow-hidden rounded-lg border bg-background">
            {isLoading ? (
              <div className="flex items-center gap-2 p-4 text-sm text-muted-foreground">
                <Loader2 className="size-4 animate-spin" aria-hidden />
                {t.backupHistory.loading}
              </div>
            ) : records.length === 0 ? (
              <div className="flex min-h-48 flex-col items-center justify-center p-6 text-center">
                <div className="flex size-10 items-center justify-center rounded-md border bg-muted/40">
                  <Archive className="size-5 text-muted-foreground" aria-hidden />
                </div>
                <div className="mt-3 text-sm font-medium">
                  {t.backupHistory.emptyTitle}
                </div>
                <p className="mt-1 max-w-sm text-xs leading-5 text-muted-foreground">
                  {t.backupHistory.emptyDescription}
                </p>
              </div>
            ) : (
              <>
                <div className="hidden overflow-x-auto md:block">
                  <table className="w-full min-w-[1040px] text-left text-sm">
                    <thead className="border-b bg-muted/40 text-xs text-muted-foreground">
                      <tr>
                        <th className="px-3 py-2 font-medium">
                          {t.backupHistory.table.id}
                        </th>
                        <th className="px-3 py-2 font-medium">
                          {t.backupHistory.table.database}
                        </th>
                        <th className="px-3 py-2 font-medium">
                          {t.backupHistory.table.status}
                        </th>
                        <th className="px-3 py-2 font-medium">
                          {t.backupHistory.table.trigger}
                        </th>
                        <th className="px-3 py-2 font-medium">
                          {t.backupHistory.table.phase}
                        </th>
                        <th className="px-3 py-2 font-medium">
                          {t.backupHistory.table.storage}
                        </th>
                        <th className="px-3 py-2 font-medium">
                          {t.backupHistory.table.size}
                        </th>
                        <th className="px-3 py-2 font-medium">
                          {t.backupHistory.table.createdAt}
                        </th>
                        <th className="px-3 py-2 font-medium">
                          {t.backupHistory.table.completedAt}
                        </th>
                      </tr>
                    </thead>
                    <tbody className="divide-y">
                      {records.map((record) => (
                        <BackupTableRow
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
                    <BackupMobileCard
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
              {t.backupHistory.pageSummary(page, pageCount, totalCount)}
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <div className="flex items-center gap-2 text-xs text-muted-foreground">
                {t.backupHistory.pageSize}
                <FilterSelect
                  value={pageSize}
                  className="w-24"
                  ariaLabel={t.backupHistory.pageSize}
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
                {t.backupHistory.previousPage}
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
                {t.backupHistory.nextPage}
                <ChevronRight className="size-4" aria-hidden />
              </Button>
            </div>
          </div>
        </CardContent>
      </Card>

      {selectedId ? (
        <BackupDetailDialog
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

function BackupTableRow({
  record,
  locale,
  onOpen,
}: {
  record: DatabaseBackupRecord;
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
        <div className="max-w-36 truncate font-medium">{record.source.name}</div>
        <div className="mt-1 max-w-44 truncate text-xs text-muted-foreground">
          {record.source.host}/{record.source.database}
        </div>
      </td>
      <td className="px-3 py-2 align-top">
        <BackupStatusBadge status={record.status} />
      </td>
      <td className="px-3 py-2 align-top">
        <BackupTriggerBadge trigger={record.trigger} />
      </td>
      <td className="px-3 py-2 align-top">
        <div className="text-xs text-muted-foreground">
          <span className="font-medium text-foreground">{record.phase}</span>
          <span className="ml-1">{record.progress_percent}%</span>
        </div>
      </td>
      <td className="px-3 py-2 align-top text-xs text-muted-foreground">
        {storageLabel(record, t.backupHistory.noValue)}
      </td>
      <td className="px-3 py-2 align-top text-xs text-muted-foreground">
        {formatBytes(record.storage?.size_bytes, t.backupHistory.noValue)}
      </td>
      <td className="px-3 py-2 align-top text-xs text-muted-foreground">
        {formatDateTime(record.created_at, locale)}
      </td>
      <td className="px-3 py-2 align-top text-xs text-muted-foreground">
        {record.completed_at
          ? formatDateTime(record.completed_at, locale)
          : t.backupHistory.noValue}
      </td>
    </tr>
  );
}

function BackupMobileCard({
  record,
  locale,
  onOpen,
}: {
  record: DatabaseBackupRecord;
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
            {record.source.name}
          </div>
          <div className="mt-1 truncate text-xs text-muted-foreground">
            {record.source.host}/{record.source.database}
          </div>
        </div>
        <BackupStatusBadge status={record.status} />
      </div>
      <div className="mt-3 flex flex-wrap gap-1.5">
        <BackupTriggerBadge trigger={record.trigger} />
        <Badge variant="outline" className="rounded-md font-mono">
          {record.phase} · {record.progress_percent}%
        </Badge>
        <Badge variant="outline" className="rounded-md">
          {formatBytes(record.storage?.size_bytes, t.backupHistory.noValue)}
        </Badge>
      </div>
      <div className="mt-2 flex items-center gap-1.5 text-xs text-muted-foreground">
        <CalendarDays className="size-3.5" aria-hidden />
        {formatDateTime(record.created_at, locale)}
      </div>
    </article>
  );
}

function BackupDetailDialog({
  record,
  isLoading,
  locale,
  onClose,
}: {
  record: DatabaseBackupRecord | null;
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
        aria-label={t.backupHistory.closeDetail}
        onClick={onClose}
      />
      <Card
        className="relative w-full max-w-5xl overflow-hidden rounded-xl py-0 shadow-xl"
        role="dialog"
        aria-modal="true"
        aria-labelledby="database-backup-detail-title"
      >
        <CardHeader className="flex flex-row items-start justify-between gap-4 border-b bg-muted/30 px-5 py-4">
          <div className="flex min-w-0 items-start gap-3">
            <span className="mt-0.5 flex size-10 shrink-0 items-center justify-center rounded-lg bg-primary text-primary-foreground shadow-sm">
              <Archive className="size-5" aria-hidden />
            </span>
            <div className="min-w-0">
              <CardTitle id="database-backup-detail-title" className="text-base">
                {t.backupHistory.detailTitle}
              </CardTitle>
              <p className="mt-1 truncate font-mono text-xs text-muted-foreground">
                {record?.id ?? t.backupHistory.loadingDetail}
              </p>
            </div>
          </div>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            aria-label={t.backupHistory.closeDetail}
            title={t.backupHistory.closeDetail}
            onClick={onClose}
          >
            <X className="size-4" aria-hidden />
          </Button>
        </CardHeader>
        <CardContent className="max-h-[calc(100vh-8rem)] overflow-x-hidden overflow-y-auto px-5 py-4">
          {isLoading || !record ? (
            <div className="flex items-center gap-2 rounded-lg border bg-background p-4 text-sm text-muted-foreground">
              <Loader2 className="size-4 animate-spin" aria-hidden />
              {t.backupHistory.loadingDetail}
            </div>
          ) : (
            <div className="grid min-w-0 gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(320px,0.8fr)]">
              <div className="min-w-0 space-y-4">
                <DetailBlock title={t.backupHistory.metadata}>
                  <div className="space-y-2">
                    <DetailRow label={t.backupHistory.table.status}>
                      <BackupStatusBadge status={record.status} />
                    </DetailRow>
                    <DetailRow label={t.backupHistory.table.trigger}>
                      <BackupTriggerBadge trigger={record.trigger} />
                    </DetailRow>
                    <DetailRow label={t.backupHistory.table.phase}>
                      <span className="font-mono">
                        {record.phase} · {record.progress_percent}%
                      </span>
                    </DetailRow>
                    <DetailRow label={t.backupHistory.purpose}>
                      {record.purpose ?? t.backupHistory.noValue}
                    </DetailRow>
                    <DetailRow label={t.backupHistory.worker}>
                      {record.worker_id ?? t.backupHistory.noValue}
                    </DetailRow>
                    <DetailRow label={t.backupHistory.format}>
                      {record.format}
                    </DetailRow>
                  </div>
                </DetailBlock>

                <DetailBlock title={t.backupHistory.databaseSnapshot}>
                  <div className="space-y-2 text-sm">
                    <DetailRow label={t.backupHistory.table.database}>
                      {record.source.name}
                    </DetailRow>
                    <DetailRow label="Host">
                      {record.source.host}:{record.source.port}
                    </DetailRow>
                    <DetailRow label="Database">
                      {record.source.database}
                    </DetailRow>
                    <DetailRow label="User">
                      {record.source.username}
                    </DetailRow>
                    <DetailRow label="SSL">{record.source.ssl_mode}</DetailRow>
                  </div>
                </DetailBlock>

                {record.error ? (
                  <DetailBlock title={t.backupHistory.error}>
                    <div className="flex gap-2 rounded-md border border-destructive/30 bg-destructive/10 p-3 text-sm text-destructive">
                      <AlertCircle className="mt-0.5 size-4 shrink-0" aria-hidden />
                      <span className="break-words">{record.error}</span>
                    </div>
                  </DetailBlock>
                ) : null}
              </div>

              <div className="min-w-0 space-y-4">
                <DetailBlock title={t.backupHistory.storage}>
                  <div className="space-y-2 text-sm">
                    <DetailRow label={t.backupHistory.storageKind}>
                      {record.storage?.kind ?? t.backupHistory.noValue}
                    </DetailRow>
                    <DetailRow label={t.backupHistory.localPath}>
                      {record.storage?.local_path ?? t.backupHistory.noValue}
                    </DetailRow>
                    <DetailRow label={t.backupHistory.s3Object}>
                      {s3ObjectLabel(record) ?? t.backupHistory.noValue}
                    </DetailRow>
                    <DetailRow label={t.backupHistory.table.size}>
                      {formatBytes(
                        record.storage?.size_bytes,
                        t.backupHistory.noValue,
                      )}
                    </DetailRow>
                    <DetailRow label={t.backupHistory.checksum}>
                      {record.storage?.checksum_sha256 ?? t.backupHistory.noValue}
                    </DetailRow>
                    <DetailRow label={t.backupHistory.postgresVersion}>
                      {record.postgres_server_version ?? t.backupHistory.noValue}
                    </DetailRow>
                    <DetailRow label={t.backupHistory.pgDumpVersion}>
                      {record.pg_dump_version ?? t.backupHistory.noValue}
                    </DetailRow>
                  </div>
                </DetailBlock>

                <DetailBlock title={t.backupHistory.scheduleContext}>
                  <div className="space-y-2 text-sm">
                    <DetailRow label={t.backupHistory.scheduleId}>
                      {record.schedule_id ?? t.backupHistory.noValue}
                    </DetailRow>
                    <DetailRow label={t.backupHistory.scheduledFor}>
                      {record.scheduled_for
                        ? formatDateTime(record.scheduled_for, locale)
                        : t.backupHistory.noValue}
                    </DetailRow>
                    <DetailRow label={t.backupHistory.conversationId}>
                      {record.conversation_id ?? t.backupHistory.noValue}
                    </DetailRow>
                    <DetailRow label={t.backupHistory.turnId}>
                      {record.created_from_turn_id ?? t.backupHistory.noValue}
                    </DetailRow>
                  </div>
                </DetailBlock>

                <DetailBlock title={t.backupHistory.timeline}>
                  <div className="space-y-2 text-sm">
                    <TimeRow
                      icon={<CalendarDays className="size-3.5" aria-hidden />}
                      label={t.backupHistory.table.createdAt}
                      value={formatDateTime(record.created_at, locale)}
                    />
                    <TimeRow
                      icon={<RefreshCw className="size-3.5" aria-hidden />}
                      label={t.backupHistory.updatedAt}
                      value={formatDateTime(record.updated_at, locale)}
                    />
                    <TimeRow
                      icon={<Clock3 className="size-3.5" aria-hidden />}
                      label={t.backupHistory.startedAt}
                      value={
                        record.started_at
                          ? formatDateTime(record.started_at, locale)
                          : t.backupHistory.noValue
                      }
                    />
                    <TimeRow
                      icon={<HardDrive className="size-3.5" aria-hidden />}
                      label={t.backupHistory.table.completedAt}
                      value={
                        record.completed_at
                          ? formatDateTime(record.completed_at, locale)
                          : t.backupHistory.noValue
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

function BackupStatusBadge({ status }: { status: DatabaseBackupStatus }) {
  const { t } = useI18n();
  const variant =
    status === "failed" || status === "deleted"
      ? "destructive"
      : status === "succeeded"
        ? "secondary"
        : "outline";

  return (
    <Badge variant={variant} className="rounded-md font-mono">
      {t.backupHistory.statuses[status]}
    </Badge>
  );
}

function BackupTriggerBadge({ trigger }: { trigger: DatabaseBackupTrigger }) {
  const { t } = useI18n();

  return (
    <Badge variant="outline" className="rounded-md font-mono">
      {t.backupHistory.triggers[trigger]}
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

function DetailBlock({
  title,
  children,
}: {
  title: string;
  children: ReactNode;
}) {
  return (
    <section className="rounded-lg border bg-background p-4">
      <h3 className="text-xs font-semibold uppercase tracking-normal text-muted-foreground">
        {title}
      </h3>
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
    <div className="grid gap-1 text-sm sm:grid-cols-[9rem_minmax(0,1fr)]">
      <div className="text-xs text-muted-foreground">{label}</div>
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
    <div className="flex items-center gap-2 text-sm">
      <span className="flex size-7 shrink-0 items-center justify-center rounded-md border bg-muted/40 text-muted-foreground">
        {icon}
      </span>
      <span className="w-28 shrink-0 text-xs text-muted-foreground">{label}</span>
      <span className="min-w-0 break-words text-foreground">{value}</span>
    </div>
  );
}

function shortId(id: string) {
  return id.length > 12 ? `${id.slice(0, 8)}...${id.slice(-4)}` : id;
}

function formatDateTime(value: string, locale: string) {
  const date = new Date(value);

  if (Number.isNaN(date.getTime())) {
    return value;
  }

  return new Intl.DateTimeFormat(locale, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(date);
}

function formatBytes(value: number | undefined, fallback: string) {
  if (typeof value !== "number" || !Number.isFinite(value) || value < 0) {
    return fallback;
  }

  const units = ["B", "KB", "MB", "GB", "TB"];
  let size = value;
  let unitIndex = 0;
  while (size >= 1024 && unitIndex < units.length - 1) {
    size /= 1024;
    unitIndex += 1;
  }

  return `${size >= 10 || unitIndex === 0 ? size.toFixed(0) : size.toFixed(1)} ${units[unitIndex]}`;
}

function storageLabel(record: DatabaseBackupRecord, fallback: string) {
  if (!record.storage) {
    return fallback;
  }

  if (record.storage.kind === "local") {
    return record.storage.local_path ?? "local";
  }

  return s3ObjectLabel(record) ?? "s3";
}

function s3ObjectLabel(record: DatabaseBackupRecord) {
  if (!record.storage || record.storage.kind !== "s3") {
    return null;
  }

  if (record.storage.bucket && record.storage.key) {
    return `${record.storage.bucket}/${record.storage.key}`;
  }

  return record.storage.bucket ?? record.storage.key ?? null;
}
