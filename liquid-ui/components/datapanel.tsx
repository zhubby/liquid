"use client";

import {
  type KeyboardEvent,
  type ReactNode,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import ReactGridLayout, {
  type Layout,
  useContainerWidth,
  verticalCompactor,
} from "react-grid-layout";
import {
  Download,
  GripVertical,
  Loader2,
  Pencil,
  RefreshCw,
  Table2,
  Trash2,
  X,
} from "lucide-react";
import { toast } from "sonner";

import { DatapanelChartRenderer } from "@/components/datapanel-chart-renderer";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  type DatapanelCardLayoutUpdate,
  type Datapanel,
  type DatapanelCard,
  type DatapanelExport,
  type ManagedDatabase,
  apiRequest,
} from "@/lib/api";
import { QueryResultTable } from "@/components/query-result-table";
import { useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

type DatapanelWorkspacePanelProps = {
  token: string;
  conversationId: string;
  selectedDatabase: ManagedDatabase;
  refreshKey: number;
};

const GRID_COLUMNS = 12;
const GRID_ROW_HEIGHT = 44;
const LAYOUT_SAVE_DELAY_MS = 600;

export function DatapanelWorkspacePanel({
  token,
  conversationId,
  selectedDatabase,
  refreshKey,
}: DatapanelWorkspacePanelProps) {
  const { t } = useI18n();
  const { width, containerRef, mounted } = useContainerWidth();
  const [panel, setPanel] = useState<Datapanel | null>(null);
  const [titleInput, setTitleInput] = useState("");
  const [descriptionInput, setDescriptionInput] = useState("");
  const [isLoading, setIsLoading] = useState(true);
  const [isSavingPanel, setIsSavingPanel] = useState(false);
  const [isSavingLayout, setIsSavingLayout] = useState(false);
  const [refreshingCardId, setRefreshingCardId] = useState<string | null>(null);
  const [deletingCardId, setDeletingCardId] = useState<string | null>(null);
  const layoutSaveRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const lastRefreshKeyRef = useRef(refreshKey);

  const loadPanel = useCallback(async (options?: { silent?: boolean }) => {
    const silent = Boolean(options?.silent);

    if (!silent) {
      setIsLoading(true);
    }

    try {
      const nextPanel = await apiRequest<Datapanel>(
        `/api/v1/chat/conversations/${conversationId}/datapanel`,
        { token },
      );
      setPanel(nextPanel);
      setTitleInput(nextPanel.title);
      setDescriptionInput(nextPanel.description ?? "");
    } catch (error) {
      toast.error(error instanceof Error ? error.message : t.dashboard.loadFailed);
    } finally {
      if (!silent) {
        setIsLoading(false);
      }
    }
  }, [conversationId, t.dashboard.loadFailed, token]);

  useEffect(() => {
    void loadPanel();

    return () => {
      if (layoutSaveRef.current) {
        clearTimeout(layoutSaveRef.current);
      }
    };
  }, [loadPanel]);

  useEffect(() => {
    if (refreshKey === lastRefreshKeyRef.current) {
      return;
    }

    lastRefreshKeyRef.current = refreshKey;
    void loadPanel({ silent: true });
  }, [loadPanel, refreshKey]);

  const layout = useMemo<Layout>(
    () =>
      panel?.cards.map((card) => ({
        i: card.id,
        x: card.layout.x,
        y: card.layout.y,
        w: card.layout.w,
        h: card.layout.h,
        minW: card.kind === "table" ? 4 : 3,
        minH: 3,
      })) ?? [],
    [panel?.cards],
  );
  const useStackedCards = mounted && width < 640;

  const savePanelMetadata = useCallback(async () => {
    if (!panel || isSavingPanel) {
      return;
    }

    const title = titleInput.trim();
    const description = descriptionInput.trim();

    if (!title) {
      setTitleInput(panel.title);
      return;
    }

    if (title === panel.title && description === (panel.description ?? "")) {
      return;
    }

    setIsSavingPanel(true);

    try {
      const updated = await apiRequest<Datapanel>(
        `/api/v1/chat/conversations/${conversationId}/datapanel`,
        {
          method: "PATCH",
          token,
          body: {
            title,
            description,
          },
        },
      );
      setPanel(updated);
      setTitleInput(updated.title);
      setDescriptionInput(updated.description ?? "");
      toast.success(t.dashboard.saved);
    } catch (error) {
      toast.error(error instanceof Error ? error.message : t.dashboard.saveFailed);
      setTitleInput(panel.title);
      setDescriptionInput(panel.description ?? "");
    } finally {
      setIsSavingPanel(false);
    }
  }, [
    conversationId,
    descriptionInput,
    isSavingPanel,
    panel,
    t.dashboard.saveFailed,
    t.dashboard.saved,
    titleInput,
    token,
  ]);

  const handleLayoutChange = useCallback(
    (nextLayout: Layout) => {
      if (!panel || nextLayout.length === 0 || sameLayout(panel, nextLayout)) {
        return;
      }

      const updates = nextLayout.map<DatapanelCardLayoutUpdate>((item) => ({
        card_id: item.i,
        layout: {
          x: item.x,
          y: item.y,
          w: item.w,
          h: item.h,
        },
      }));

      setPanel((current) =>
        current
          ? {
              ...current,
              cards: current.cards.map((card) => {
                const update = updates.find((item) => item.card_id === card.id);

                return update ? { ...card, layout: update.layout } : card;
              }),
            }
          : current,
      );

      if (layoutSaveRef.current) {
        clearTimeout(layoutSaveRef.current);
      }

      setIsSavingLayout(true);
      layoutSaveRef.current = setTimeout(() => {
        void apiRequest<Datapanel>(`/api/v1/datapanels/${panel.id}/layout`, {
          method: "PATCH",
          token,
          body: { cards: updates },
        })
          .then((updated) => {
            setPanel(updated);
          })
          .catch((error) => {
            toast.error(
              error instanceof Error ? error.message : t.dashboard.layoutSaveFailed,
            );
            void loadPanel();
          })
          .finally(() => {
            setIsSavingLayout(false);
          });
      }, LAYOUT_SAVE_DELAY_MS);
    },
    [loadPanel, panel, t.dashboard.layoutSaveFailed, token],
  );

  const updateCard = useCallback(
    async (
      cardId: string,
      request: { title?: string; description?: string },
    ) => {
      if (!panel) {
        return;
      }

      try {
        const updated = await apiRequest<DatapanelCard>(
          `/api/v1/datapanels/${panel.id}/cards/${cardId}`,
          {
            method: "PATCH",
            token,
            body: request,
          },
        );

        setPanel((current) =>
          current
            ? {
                ...current,
                cards: current.cards.map((card) =>
                  card.id === updated.id ? updated : card,
                ),
              }
            : current,
        );
        toast.success(t.dashboard.saved);
      } catch (error) {
        toast.error(error instanceof Error ? error.message : t.dashboard.saveFailed);
      }
    },
    [panel, t.dashboard.saveFailed, t.dashboard.saved, token],
  );

  const refreshCard = useCallback(
    async (cardId: string) => {
      if (!panel || refreshingCardId) {
        return;
      }

      setRefreshingCardId(cardId);

      try {
        const updated = await apiRequest<DatapanelCard>(
          `/api/v1/datapanels/${panel.id}/cards/${cardId}/refresh`,
          {
            method: "POST",
            token,
            body: {},
          },
        );

        setPanel((current) =>
          current
            ? {
                ...current,
                cards: current.cards.map((card) =>
                  card.id === updated.id ? updated : card,
                ),
              }
            : current,
        );
        toast.success(t.dashboard.refreshed);
      } catch (error) {
        toast.error(error instanceof Error ? error.message : t.dashboard.refreshFailed);
      } finally {
        setRefreshingCardId(null);
      }
    },
    [panel, refreshingCardId, t.dashboard.refreshFailed, t.dashboard.refreshed, token],
  );

  const deleteCard = useCallback(
    async (cardId: string) => {
      if (!panel || deletingCardId) {
        return;
      }

      setDeletingCardId(cardId);

      try {
        await apiRequest<void>(`/api/v1/datapanels/${panel.id}/cards/${cardId}`, {
          method: "DELETE",
          token,
        });
        setPanel((current) =>
          current
            ? {
                ...current,
                cards: current.cards.filter((card) => card.id !== cardId),
              }
            : current,
        );
        toast.success(t.dashboard.deleted);
      } catch (error) {
        toast.error(error instanceof Error ? error.message : t.dashboard.deleteFailed);
      } finally {
        setDeletingCardId(null);
      }
    },
    [deletingCardId, panel, t.dashboard.deleteFailed, t.dashboard.deleted, token],
  );

  const exportPanel = useCallback(async () => {
    if (!panel) {
      return;
    }

    try {
      const exported = await apiRequest<DatapanelExport>(
        `/api/v1/datapanels/${panel.id}/export`,
        { token },
      );
      downloadJson(`${safeFileName(panel.title)}.json`, exported);
      toast.success(t.dashboard.exported);
    } catch (error) {
      toast.error(error instanceof Error ? error.message : t.dashboard.exportFailed);
    }
  }, [panel, t.dashboard.exportFailed, t.dashboard.exported, token]);

  return (
    <section className="mt-3 flex min-h-[calc(100vh-1.5rem)] min-w-0 flex-col overflow-hidden rounded-lg border bg-card text-card-foreground shadow-sm lg:mt-0 lg:h-[calc(100vh-1.5rem)]">
      <header className="flex shrink-0 flex-col gap-3 border-b px-4 py-3 xl:flex-row xl:items-center xl:justify-between">
        <div className="min-w-0 flex-1">
          <div className="flex min-w-0 items-center gap-2">
            <input
              className="min-w-0 flex-1 truncate rounded-sm bg-transparent text-base font-semibold outline-none transition-colors hover:bg-muted/50 focus-visible:bg-background focus-visible:ring-[3px] focus-visible:ring-ring/50 disabled:opacity-70"
              value={titleInput}
              disabled={isLoading || isSavingPanel}
              aria-label={t.dashboard.panelTitleLabel}
              onChange={(event) => setTitleInput(event.target.value)}
              onBlur={() => void savePanelMetadata()}
              onKeyDown={(event) => handleCommitKey(event, savePanelMetadata)}
            />
            {isSavingPanel || isSavingLayout ? (
              <Loader2 className="size-4 shrink-0 animate-spin text-muted-foreground" />
            ) : null}
          </div>
          <input
            className="mt-1 w-full min-w-0 truncate rounded-sm bg-transparent text-xs text-muted-foreground outline-none transition-colors hover:bg-muted/50 focus-visible:bg-background focus-visible:ring-[3px] focus-visible:ring-ring/50"
            value={descriptionInput}
            placeholder={`${selectedDatabase.host}:${selectedDatabase.port} / ${selectedDatabase.database}`}
            aria-label={t.dashboard.panelDescriptionLabel}
            onChange={(event) => setDescriptionInput(event.target.value)}
            onBlur={() => void savePanelMetadata()}
            onKeyDown={(event) => handleCommitKey(event, savePanelMetadata)}
          />
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={!panel}
            onClick={() => void exportPanel()}
          >
            <Download className="size-4" aria-hidden />
            {t.dashboard.export}
          </Button>
        </div>
      </header>

      <div
        ref={containerRef}
        className="min-h-0 flex-1 overflow-y-auto px-3 py-3"
      >
        {isLoading ? (
          <div className="flex min-h-full items-center justify-center text-sm text-muted-foreground">
            <Loader2 className="mr-2 size-4 animate-spin" aria-hidden />
            {t.dashboard.loading}
          </div>
        ) : null}

        {!isLoading && panel && panel.cards.length === 0 ? (
          <BiEmptyState />
        ) : null}

        {!isLoading && panel && panel.cards.length > 0 && mounted && useStackedCards ? (
          <div className="space-y-3">
            {panel.cards.map((card) => (
              <div key={card.id} className="h-[360px] min-h-0 min-w-0">
                <DatapanelCard
                  card={card}
                  isRefreshing={refreshingCardId === card.id}
                  isDeleting={deletingCardId === card.id}
                  isStacked
                  onUpdate={updateCard}
                  onRefresh={() => void refreshCard(card.id)}
                  onDelete={() => void deleteCard(card.id)}
                />
              </div>
            ))}
          </div>
        ) : null}

        {!isLoading && panel && panel.cards.length > 0 && mounted && !useStackedCards ? (
          <ReactGridLayout
            width={width}
            layout={layout}
            autoSize
            gridConfig={{
              cols: GRID_COLUMNS,
              rowHeight: GRID_ROW_HEIGHT,
              margin: [12, 12],
              containerPadding: [0, 0],
            }}
            dragConfig={{
              enabled: true,
              handle: ".datapanel-card-drag-handle",
              cancel: ".datapanel-card-no-drag",
              bounded: true,
            }}
            resizeConfig={{
              enabled: true,
              handles: ["se"],
            }}
            compactor={verticalCompactor}
            onLayoutChange={handleLayoutChange}
          >
            {panel.cards.map((card) => (
              <div key={card.id} className="min-w-0">
                <DatapanelCard
                  card={card}
                  isRefreshing={refreshingCardId === card.id}
                  isDeleting={deletingCardId === card.id}
                  isStacked={false}
                  onUpdate={updateCard}
                  onRefresh={() => void refreshCard(card.id)}
                  onDelete={() => void deleteCard(card.id)}
                />
              </div>
            ))}
          </ReactGridLayout>
        ) : null}
      </div>
    </section>
  );
}

function BiEmptyState() {
  const { t } = useI18n();

  return (
    <div className="flex min-h-full items-center justify-center py-12">
      <div className="w-full max-w-sm text-center">
        <div className="mx-auto flex size-10 items-center justify-center rounded-md border bg-secondary text-secondary-foreground">
          <Table2 className="size-5" aria-hidden />
        </div>
        <h2 className="mt-4 text-base font-semibold">
          {t.dashboard.emptyTitle}
        </h2>
        <p className="mt-2 text-sm leading-6 text-muted-foreground">
          {t.dashboard.emptyDescription}
        </p>
      </div>
    </div>
  );
}

function DatapanelCard({
  card,
  isRefreshing,
  isDeleting,
  isStacked,
  onUpdate,
  onRefresh,
  onDelete,
}: {
  card: DatapanelCard;
  isRefreshing: boolean;
  isDeleting: boolean;
  isStacked: boolean;
  onUpdate: (
    cardId: string,
    request: { title?: string; description?: string },
  ) => void | Promise<void>;
  onRefresh: () => void;
  onDelete: () => void;
}) {
  const { t } = useI18n();
  const [isEditing, setIsEditing] = useState(false);
  const [titleInput, setTitleInput] = useState(card.title);
  const [descriptionInput, setDescriptionInput] = useState(card.description ?? "");

  useEffect(() => {
    setTitleInput(card.title);
    setDescriptionInput(card.description ?? "");
  }, [card.description, card.title]);

  const commit = useCallback(() => {
    const title = titleInput.trim();

    if (!title) {
      setTitleInput(card.title);
      return;
    }

    void onUpdate(card.id, {
      title,
      description: descriptionInput.trim(),
    });
    setIsEditing(false);
  }, [card.id, card.title, descriptionInput, onUpdate, titleInput]);

  return (
    <article className="flex h-full min-h-0 min-w-0 flex-col overflow-hidden rounded-lg border bg-background shadow-xs">
      <header
        className={cn(
          "datapanel-card-drag-handle flex shrink-0 items-start gap-2 border-b bg-muted/40 px-3 py-2",
          isStacked ? "cursor-default" : "cursor-move",
        )}
      >
        <GripVertical className="mt-1 size-4 shrink-0 text-muted-foreground" aria-hidden />
        <div className="min-w-0 flex-1">
          <input
            className="datapanel-card-no-drag w-full truncate rounded-sm bg-transparent text-sm font-medium outline-none hover:bg-background/70 focus-visible:bg-background focus-visible:ring-[3px] focus-visible:ring-ring/50"
            value={titleInput}
            aria-label={t.dashboard.cardTitleLabel}
            onChange={(event) => setTitleInput(event.target.value)}
            onBlur={commit}
            onKeyDown={(event) => handleCommitKey(event, commit)}
          />
          {isEditing ? (
            <textarea
              className="datapanel-card-no-drag mt-1 max-h-16 min-h-8 w-full resize-none rounded-sm bg-background px-2 py-1 text-xs leading-5 text-muted-foreground outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50"
              value={descriptionInput}
              aria-label={t.dashboard.cardDescriptionLabel}
              onChange={(event) => setDescriptionInput(event.target.value)}
              onBlur={commit}
            />
          ) : (
            <p className="mt-0.5 truncate text-xs text-muted-foreground">
              {card.description || t.dashboard.sqlSource(card.result.row_count)}
            </p>
          )}
        </div>
        <div className="datapanel-card-no-drag flex shrink-0 items-center gap-1">
          <Badge variant="outline" className="h-6 rounded-md">
            {t.dashboard.cardKinds[card.kind]}
          </Badge>
          <IconButton
            label={t.dashboard.refresh}
            onClick={onRefresh}
            disabled={isRefreshing}
          >
            {isRefreshing ? (
              <Loader2 className="size-4 animate-spin" aria-hidden />
            ) : (
              <RefreshCw className="size-4" aria-hidden />
            )}
          </IconButton>
          <IconButton
            label={isEditing ? t.common.close : t.dashboard.editCard}
            onClick={() => setIsEditing((current) => !current)}
          >
            {isEditing ? (
              <X className="size-4" aria-hidden />
            ) : (
              <Pencil className="size-4" aria-hidden />
            )}
          </IconButton>
          <IconButton
            label={t.dashboard.deleteCard}
            onClick={onDelete}
            disabled={isDeleting}
            destructive
          >
            {isDeleting ? (
              <Loader2 className="size-4 animate-spin" aria-hidden />
            ) : (
              <Trash2 className="size-4" aria-hidden />
            )}
          </IconButton>
        </div>
      </header>
      <div className="min-h-0 flex-1 overflow-hidden px-3 py-3">
        {card.kind === "chart" && card.chart ? (
          <DatapanelChart card={card} />
        ) : (
          <DatapanelTable card={card} />
        )}
      </div>
    </article>
  );
}

function IconButton({
  label,
  children,
  disabled = false,
  destructive = false,
  onClick,
}: {
  label: string;
  children: ReactNode;
  disabled?: boolean;
  destructive?: boolean;
  onClick: () => void;
}) {
  return (
    <Button
      type="button"
      variant="ghost"
      size="icon"
      className={cn(
        "size-8 rounded-md",
        destructive && "text-destructive hover:bg-destructive/10 hover:text-destructive",
      )}
      disabled={disabled}
      title={label}
      aria-label={label}
      onClick={onClick}
    >
      {children}
    </Button>
  );
}

function DatapanelTable({ card }: { card: DatapanelCard }) {
  const { t } = useI18n();

  return (
    <QueryResultTable
      result={card.result}
      emptyLabel={t.dashboard.noRows}
    />
  );
}

function DatapanelChart({ card }: { card: DatapanelCard }) {
  const { t } = useI18n();
  const chart = card.chart;

  if (!chart) {
    return <DatapanelTable card={card} />;
  }

  return (
    <DatapanelChartRenderer
      chart={chart}
      rows={card.result.rows}
      emptyLabel={t.dashboard.noRows}
    />
  );
}

function handleCommitKey(
  event: KeyboardEvent<HTMLInputElement | HTMLTextAreaElement>,
  commit: () => void | Promise<void>,
) {
  if (event.key === "Enter" && event.currentTarget.tagName !== "TEXTAREA") {
    event.preventDefault();
    void commit();
    event.currentTarget.blur();
  }

  if (event.key === "Escape") {
    event.currentTarget.blur();
  }
}

function sameLayout(panel: Datapanel, layout: Layout) {
  return panel.cards.every((card) => {
    const item = layout.find((item) => item.i === card.id);

    return (
      item &&
      item.x === card.layout.x &&
      item.y === card.layout.y &&
      item.w === card.layout.w &&
      item.h === card.layout.h
    );
  });
}

function safeFileName(value: string) {
  return value.trim().replace(/[^a-z0-9-_]+/gi, "-").replace(/^-|-$/g, "") || "datapanel";
}

function downloadJson(fileName: string, value: unknown) {
  const blob = new Blob([JSON.stringify(value, null, 2)], {
    type: "application/json",
  });
  const url = window.URL.createObjectURL(blob);
  const anchor = document.createElement("a");

  anchor.href = url;
  anchor.download = fileName;
  anchor.click();
  window.URL.revokeObjectURL(url);
}
