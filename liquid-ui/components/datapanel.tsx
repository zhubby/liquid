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
  type EventCallback,
  type Layout,
  verticalCompactor,
  useContainerWidth,
} from "react-grid-layout";
import {
  BarChart3,
  Download,
  Eye,
  GripVertical,
  Loader2,
  RefreshCw,
  Table2,
  Trash2,
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
  type DatapanelPreview,
  type DatapanelPreviewCard,
  type DatapanelPreviewLink,
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

type DatapanelDisplayCard = {
  renderId: string;
  title: string;
  description?: string;
  kind: DatapanelCard["kind"];
  chart?: DatapanelCard["chart"];
  layout: DatapanelCard["layout"];
  result: DatapanelCard["result"];
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
  const [panel, setPanel] = useState<Datapanel | null>(null);
  const [titleInput, setTitleInput] = useState("");
  const [descriptionInput, setDescriptionInput] = useState("");
  const [isLoading, setIsLoading] = useState(true);
  const [isSavingPanel, setIsSavingPanel] = useState(false);
  const [isSavingLayout, setIsSavingLayout] = useState(false);
  const [isOpeningPreview, setIsOpeningPreview] = useState(false);
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

  const openPreview = useCallback(async () => {
    if (!panel || isOpeningPreview) {
      return;
    }

    const previewWindow = window.open("about:blank", "_blank");
    if (previewWindow) {
      previewWindow.opener = null;
    }

    setIsOpeningPreview(true);

    try {
      const preview = await apiRequest<DatapanelPreviewLink>(
        `/api/v1/datapanels/${panel.id}/preview`,
        {
          method: "POST",
          token,
        },
      );
      const previewPath = `/preview/datapanels/${encodeURIComponent(preview.slug)}`;

      if (previewWindow) {
        previewWindow.location.href = previewPath;
      } else {
        window.open(previewPath, "_blank", "noopener,noreferrer");
      }
    } catch (error) {
      if (previewWindow && !previewWindow.closed) {
        previewWindow.close();
      }
      toast.error(error instanceof Error ? error.message : t.dashboard.previewFailed);
    } finally {
      setIsOpeningPreview(false);
    }
  }, [isOpeningPreview, panel, t.dashboard.previewFailed, token]);

  return (
    <section className="mt-3 flex min-h-[calc(100vh-1.5rem)] min-w-0 flex-col overflow-hidden rounded-lg border bg-card text-card-foreground shadow-sm lg:mt-0 lg:h-[calc(100vh-1.5rem)]">
      <header className="flex shrink-0 flex-col gap-3 border-b px-4 py-3 xl:flex-row xl:items-center xl:justify-between">
        <div className="flex min-w-0 flex-1 items-start gap-3">
          <span className="mt-0.5 flex size-9 shrink-0 items-center justify-center rounded-md border bg-secondary text-secondary-foreground">
            <BarChart3 className="size-4" aria-hidden />
          </span>
          <div className="min-w-0 flex-1">
            <div className="flex min-w-0 items-center gap-2">
              <input
                className="min-w-0 flex-1 truncate rounded-sm bg-transparent text-base font-semibold outline-none transition-colors hover:bg-muted/50 focus-visible:bg-background focus-visible:ring-[3px] focus-visible:ring-ring/50 disabled:opacity-70"
                value={titleInput}
                disabled={isLoading || isSavingPanel}
                aria-label={t.dashboard.panelTitleLabel}
                onChange={(event) => setTitleInput(event.target.value)}
                onBlur={() => void savePanelMetadata()}
                onKeyDown={handleCommitKey}
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
              onKeyDown={handleCommitKey}
            />
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button
            type="button"
            variant="outline"
            size="icon"
            className="size-9"
            aria-label={t.dashboard.preview}
            title={t.dashboard.preview}
            disabled={!panel || isOpeningPreview}
            onClick={() => void openPreview()}
          >
            {isOpeningPreview ? (
              <Loader2 className="size-4 animate-spin" aria-hidden />
            ) : (
              <Eye className="size-4" aria-hidden />
            )}
          </Button>
          <Button
            type="button"
            variant="outline"
            size="icon"
            className="size-9"
            aria-label={t.dashboard.export}
            title={t.dashboard.export}
            disabled={!panel}
            onClick={() => void exportPanel()}
          >
            <Download className="size-4" aria-hidden />
          </Button>
        </div>
      </header>

      <DatapanelGrid
        cards={panel?.cards.map(datapanelCardToDisplayCard) ?? []}
        isLoading={isLoading}
        showEmpty={Boolean(panel && panel.cards.length === 0)}
        refreshingCardId={refreshingCardId}
        deletingCardId={deletingCardId}
        onLayoutChange={handleLayoutChange}
        onUpdate={updateCard}
        onRefresh={refreshCard}
        onDelete={deleteCard}
      />
    </section>
  );
}

export function DatapanelReadonlyPanel({
  preview,
  className,
}: {
  preview: DatapanelPreview;
  className?: string;
}) {
  const { t } = useI18n();

  return (
    <section
      className={cn(
        "flex min-h-[calc(100vh-6.5rem)] min-w-0 flex-col overflow-hidden rounded-lg border bg-card text-card-foreground shadow-sm",
        className,
      )}
    >
      <header className="flex shrink-0 flex-col gap-2 border-b px-4 py-3">
        <div className="flex min-w-0 items-center justify-between gap-3">
          <div className="min-w-0">
            <h2 className="truncate text-base font-semibold">{preview.title}</h2>
            {preview.description ? (
              <p className="mt-1 truncate text-xs text-muted-foreground">
                {preview.description}
              </p>
            ) : null}
          </div>
          <Badge variant="secondary" className="shrink-0 rounded-md">
            {t.dashboard.readOnlyPreview}
          </Badge>
        </div>
      </header>
      <DatapanelGrid
        cards={preview.cards.map(datapanelPreviewCardToDisplayCard)}
        isLoading={false}
        showEmpty={preview.cards.length === 0}
        readOnly
        emptyTitle={t.dashboard.previewEmptyTitle}
        emptyDescription={t.dashboard.previewEmptyDescription}
      />
    </section>
  );
}

function DatapanelGrid({
  cards,
  isLoading,
  showEmpty,
  readOnly = false,
  emptyTitle,
  emptyDescription,
  refreshingCardId = null,
  deletingCardId = null,
  onLayoutChange,
  onUpdate,
  onRefresh,
  onDelete,
}: {
  cards: DatapanelDisplayCard[];
  isLoading: boolean;
  showEmpty: boolean;
  readOnly?: boolean;
  emptyTitle?: string;
  emptyDescription?: string;
  refreshingCardId?: string | null;
  deletingCardId?: string | null;
  onLayoutChange?: (layout: Layout) => void;
  onUpdate?: (
    cardId: string,
    request: { title?: string; description?: string },
  ) => void | Promise<void>;
  onRefresh?: (cardId: string) => void;
  onDelete?: (cardId: string) => void;
}) {
  const { t } = useI18n();
  const { width, containerRef, mounted } = useContainerWidth();
  const [activeInteractionCardId, setActiveInteractionCardId] = useState<
    string | null
  >(null);
  const interactionFrameRef = useRef<number | null>(null);
  const layout = useMemo<Layout>(
    () =>
      cards.map((card) => ({
        i: card.renderId,
        x: card.layout.x,
        y: card.layout.y,
        w: card.layout.w,
        h: card.layout.h,
        minW: card.kind === "table" ? 4 : 3,
        minH: 3,
      })),
    [cards],
  );
  const useStackedCards = mounted && width < 640;
  const handleInteractionStart = useCallback<EventCallback>(
    (_layout, oldItem, item) => {
      if (interactionFrameRef.current !== null) {
        window.cancelAnimationFrame(interactionFrameRef.current);
        interactionFrameRef.current = null;
      }

      setActiveInteractionCardId(item?.i ?? oldItem?.i ?? null);
    },
    [],
  );
  const handleInteractionStop = useCallback(() => {
    interactionFrameRef.current = window.requestAnimationFrame(() => {
      setActiveInteractionCardId(null);
      interactionFrameRef.current = null;
    });
  }, []);

  useEffect(() => {
    return () => {
      if (interactionFrameRef.current !== null) {
        window.cancelAnimationFrame(interactionFrameRef.current);
      }
    };
  }, []);

  return (
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

      {!isLoading && showEmpty ? (
        <DatapanelEmptyState title={emptyTitle} description={emptyDescription} />
      ) : null}

      {!isLoading && cards.length > 0 && mounted && useStackedCards ? (
        <div className="space-y-3">
          {cards.map((card) => (
            <div key={card.renderId} className="h-[360px] min-h-0 min-w-0">
              <DatapanelCardView
                card={card}
                isRefreshing={refreshingCardId === card.renderId}
                isDeleting={deletingCardId === card.renderId}
                isStacked
                isInteracting={false}
                readOnly={readOnly}
                onUpdate={onUpdate}
                onRefresh={onRefresh ? () => onRefresh(card.renderId) : undefined}
                onDelete={onDelete ? () => onDelete(card.renderId) : undefined}
              />
            </div>
          ))}
        </div>
      ) : null}

      {!isLoading && cards.length > 0 && mounted && !useStackedCards ? (
        <ReactGridLayout
          className={cn(
            "datapanel-grid-layout",
            activeInteractionCardId && "datapanel-grid-layout-interacting",
          )}
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
            enabled: !readOnly,
            handle: ".datapanel-card-drag-handle",
            cancel: ".datapanel-card-no-drag",
            bounded: true,
          }}
          resizeConfig={{
            enabled: !readOnly,
            handles: ["se"],
          }}
          compactor={verticalCompactor}
          onDragStart={readOnly ? undefined : handleInteractionStart}
          onDragStop={readOnly ? undefined : handleInteractionStop}
          onResizeStart={readOnly ? undefined : handleInteractionStart}
          onResizeStop={readOnly ? undefined : handleInteractionStop}
          onLayoutChange={readOnly ? undefined : onLayoutChange}
        >
          {cards.map((card) => (
            <div key={card.renderId} className="min-w-0">
              <DatapanelCardView
                card={card}
                isRefreshing={refreshingCardId === card.renderId}
                isDeleting={deletingCardId === card.renderId}
                isStacked={false}
                isInteracting={activeInteractionCardId === card.renderId}
                readOnly={readOnly}
                onUpdate={onUpdate}
                onRefresh={onRefresh ? () => onRefresh(card.renderId) : undefined}
                onDelete={onDelete ? () => onDelete(card.renderId) : undefined}
              />
            </div>
          ))}
        </ReactGridLayout>
      ) : null}
    </div>
  );
}

function DatapanelEmptyState({
  title,
  description,
}: {
  title?: string;
  description?: string;
}) {
  const { t } = useI18n();

  return (
    <div className="flex min-h-full items-center justify-center py-12">
      <div className="w-full max-w-sm text-center">
        <div className="mx-auto flex size-10 items-center justify-center rounded-md border bg-secondary text-secondary-foreground">
          <Table2 className="size-5" aria-hidden />
        </div>
        <h2 className="mt-4 text-base font-semibold">
          {title ?? t.dashboard.emptyTitle}
        </h2>
        <p className="mt-2 text-sm leading-6 text-muted-foreground">
          {description ?? t.dashboard.emptyDescription}
        </p>
      </div>
    </div>
  );
}

function DatapanelCardView({
  card,
  isRefreshing,
  isDeleting,
  isStacked,
  isInteracting,
  readOnly,
  onUpdate,
  onRefresh,
  onDelete,
}: {
  card: DatapanelDisplayCard;
  isRefreshing: boolean;
  isDeleting: boolean;
  isStacked: boolean;
  isInteracting: boolean;
  readOnly: boolean;
  onUpdate?: (
    cardId: string,
    request: { title?: string; description?: string },
  ) => void | Promise<void>;
  onRefresh?: () => void;
  onDelete?: () => void;
}) {
  const { t } = useI18n();
  const [titleInput, setTitleInput] = useState(card.title);
  const [descriptionInput, setDescriptionInput] = useState(card.description ?? "");

  useEffect(() => {
    setTitleInput(card.title);
    setDescriptionInput(card.description ?? "");
  }, [card.description, card.title]);

  const commit = useCallback(() => {
    if (readOnly || !onUpdate) {
      return;
    }

    const title = titleInput.trim();

    if (!title) {
      setTitleInput(card.title);
      return;
    }

    void onUpdate(card.renderId, {
      title,
      description: descriptionInput.trim(),
    });
  }, [card.renderId, card.title, descriptionInput, onUpdate, readOnly, titleInput]);

  return (
    <article
      className={cn(
        "flex h-full min-h-0 min-w-0 flex-col overflow-hidden rounded-lg border bg-background shadow-xs transition-[box-shadow,transform,border-color,opacity] duration-150 will-change-transform",
        isInteracting &&
          "scale-[0.995] select-none border-foreground/15 shadow-lg shadow-foreground/10",
      )}
    >
      <header
        className={cn(
          "flex shrink-0 items-start gap-2 border-b bg-muted/40 px-3 py-2",
          !readOnly && "datapanel-card-drag-handle",
          readOnly || isStacked ? "cursor-default" : "cursor-move",
        )}
      >
        {!readOnly ? (
          <GripVertical className="mt-1 size-4 shrink-0 text-muted-foreground" aria-hidden />
        ) : null}
        <div className="min-w-0 flex-1">
          {readOnly ? (
            <h3 className="truncate text-sm font-medium">{card.title}</h3>
          ) : (
            <input
              className="datapanel-card-no-drag w-full truncate rounded-sm bg-transparent text-sm font-medium outline-none hover:bg-background/70 focus-visible:bg-background focus-visible:ring-[3px] focus-visible:ring-ring/50"
              value={titleInput}
              aria-label={t.dashboard.cardTitleLabel}
              onChange={(event) => setTitleInput(event.target.value)}
              onBlur={commit}
              onKeyDown={handleCommitKey}
            />
          )}
          {readOnly ? (
            <p className="mt-0.5 truncate text-xs text-muted-foreground">
              {card.description || t.dashboard.sqlSource(card.result.row_count)}
            </p>
          ) : (
            <input
              className="datapanel-card-no-drag mt-0.5 w-full truncate rounded-sm bg-transparent text-xs text-muted-foreground outline-none hover:bg-background/70 focus-visible:bg-background focus-visible:ring-[3px] focus-visible:ring-ring/50"
              value={descriptionInput}
              placeholder={t.dashboard.sqlSource(card.result.row_count)}
              aria-label={t.dashboard.cardDescriptionLabel}
              onChange={(event) => setDescriptionInput(event.target.value)}
              onBlur={commit}
              onKeyDown={handleCommitKey}
            />
          )}
        </div>
        <div className="datapanel-card-no-drag flex shrink-0 items-center gap-1">
          <Badge variant="outline" className="h-6 rounded-md">
            {t.dashboard.cardKinds[card.kind]}
          </Badge>
          {!readOnly && onRefresh ? (
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
          ) : null}
          {!readOnly && onDelete ? (
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
          ) : null}
        </div>
      </header>
      <div
        className={cn(
          "datapanel-card-content min-h-0 flex-1 overflow-hidden px-3 py-3 transition-opacity duration-100",
          isInteracting && "pointer-events-none opacity-35",
        )}
      >
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

function DatapanelTable({ card }: { card: DatapanelDisplayCard }) {
  const { t } = useI18n();

  return (
    <QueryResultTable
      result={card.result}
      emptyLabel={t.dashboard.noRows}
    />
  );
}

function DatapanelChart({ card }: { card: DatapanelDisplayCard }) {
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

function datapanelCardToDisplayCard(card: DatapanelCard): DatapanelDisplayCard {
  return {
    renderId: card.id,
    title: card.title,
    description: card.description,
    kind: card.kind,
    chart: card.chart,
    layout: card.layout,
    result: card.result,
  };
}

function datapanelPreviewCardToDisplayCard(
  card: DatapanelPreviewCard,
  index: number,
): DatapanelDisplayCard {
  return {
    renderId: `preview-card-${index}`,
    title: card.title,
    description: card.description,
    kind: card.kind,
    chart: card.chart,
    layout: card.layout,
    result: card.result,
  };
}

function handleCommitKey(
  event: KeyboardEvent<HTMLInputElement | HTMLTextAreaElement>,
) {
  if (event.key === "Enter" && event.currentTarget.tagName !== "TEXTAREA") {
    event.preventDefault();
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
