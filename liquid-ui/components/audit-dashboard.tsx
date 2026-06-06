"use client";

import {
  type CSSProperties,
  type MouseEvent,
  type MouseEventHandler,
  type PointerEvent,
  type ReactNode,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { Bot, Loader2, LogOut, Plus, Sparkles } from "lucide-react";
import { toast } from "sonner";

import { BiWorkspacePanel } from "@/components/bi-panel";
import { ChatPanel } from "@/components/chat-workspace";
import { Button } from "@/components/ui/button";
import {
  type ChatConversation,
  type ManagedDatabase,
  type PublicUser,
  apiRequest,
} from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

type AuditDashboardProps = {
  token: string;
  user: PublicUser;
  selectedDatabase: ManagedDatabase;
  onDatabaseExit: () => void;
};

const MIN_AI_WIDTH = 320;
const MIN_BI_WIDTH = 520;
const DEFAULT_AI_PERCENT = 38;

function newWorkspaceTitle(prefix: string) {
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
  const suffix = Array.from({ length: 5 }, () =>
    alphabet[Math.floor(Math.random() * alphabet.length)],
  ).join("");

  return `${prefix}-${suffix}`;
}

export function AuditDashboard({
  token,
  user,
  selectedDatabase,
  onDatabaseExit,
}: AuditDashboardProps) {
  const { t } = useI18n();
  const workspaceRef = useRef<HTMLDivElement>(null);
  const [aiWidth, setAiWidth] = useState(DEFAULT_AI_PERCENT);
  const [isDragging, setIsDragging] = useState(false);
  const [conversations, setConversations] = useState<ChatConversation[]>([]);
  const [activeConversation, setActiveConversation] =
    useState<ChatConversation | null>(null);
  const [isWorkspaceLoading, setIsWorkspaceLoading] = useState(true);
  const [isCreatingWorkspace, setIsCreatingWorkspace] = useState(false);
  const [isDeletingWorkspace, setIsDeletingWorkspace] = useState(false);

  const workspaceStyle = useMemo(
    () =>
      ({
        "--ai-pane-width": `${aiWidth}%`,
      }) as CSSProperties,
    [aiWidth],
  );

  useEffect(() => {
    let cancelled = false;
    const conversationsPath = `/api/v1/chat/conversations?managed_database_id=${encodeURIComponent(
      selectedDatabase.id,
    )}`;
    const createWorkspaceBody = () => ({
      title: newWorkspaceTitle(t.workspace.defaultTitlePrefix),
      managed_database_id: selectedDatabase.id,
    });

    const initializeWorkspaces = async () => {
      setIsWorkspaceLoading(true);

      try {
        const existingConversations = await apiRequest<ChatConversation[]>(
          conversationsPath,
          { token },
        );
        const initialConversation =
          existingConversations[0] ??
          (await apiRequest<ChatConversation>("/api/v1/chat/conversations", {
            method: "POST",
            token,
            body: createWorkspaceBody(),
          }));

        if (cancelled) {
          return;
        }

        setConversations(
          existingConversations.length > 0
            ? existingConversations
            : [initialConversation],
        );
        setActiveConversation(initialConversation);
      } catch (error) {
        if (!cancelled) {
          toast.error(
            error instanceof Error ? error.message : t.workspace.loadFailed,
          );
        }
      } finally {
        if (!cancelled) {
          setIsWorkspaceLoading(false);
        }
      }
    };

    void initializeWorkspaces();

    return () => {
      cancelled = true;
    };
  }, [
    selectedDatabase.id,
    t.workspace.defaultTitlePrefix,
    t.workspace.loadFailed,
    token,
  ]);

  const handleCreateWorkspace = useCallback(async () => {
    if (isCreatingWorkspace) {
      return;
    }

    setIsCreatingWorkspace(true);

    try {
      const nextConversation = await apiRequest<ChatConversation>(
        "/api/v1/chat/conversations",
        {
          method: "POST",
          token,
          body: {
            title: newWorkspaceTitle(t.workspace.defaultTitlePrefix),
            managed_database_id: selectedDatabase.id,
          },
        },
      );

      setConversations((current) => [
        nextConversation,
        ...current.filter(
          (conversation) => conversation.id !== nextConversation.id,
        ),
      ]);
      setActiveConversation(nextConversation);
    } catch (error) {
      toast.error(
        error instanceof Error ? error.message : t.workspace.createFailed,
      );
    } finally {
      setIsCreatingWorkspace(false);
    }
  }, [
    isCreatingWorkspace,
    selectedDatabase.id,
    t.workspace.createFailed,
    t.workspace.defaultTitlePrefix,
    token,
  ]);

  const handleConversationUpdated = useCallback(
    (updatedConversation: ChatConversation) => {
      setConversations((current) =>
        current.map((conversation) =>
          conversation.id === updatedConversation.id
            ? updatedConversation
            : conversation,
        ),
      );
      setActiveConversation((current) =>
        current?.id === updatedConversation.id ? updatedConversation : current,
      );
    },
    [],
  );

  const handleDeleteWorkspace = useCallback(
    async (conversationId: string) => {
      if (isDeletingWorkspace) {
        return;
      }

      setIsDeletingWorkspace(true);

      try {
        await apiRequest<void>(`/api/v1/chat/conversations/${conversationId}`, {
          method: "DELETE",
          token,
        });

        const remainingConversations = conversations.filter(
          (conversation) => conversation.id !== conversationId,
        );
        let nextConversations = remainingConversations;
        let nextActiveConversation =
          activeConversation?.id === conversationId
            ? remainingConversations[0] ?? null
            : activeConversation;

        if (nextConversations.length === 0) {
          const replacementConversation = await apiRequest<ChatConversation>(
            "/api/v1/chat/conversations",
            {
              method: "POST",
              token,
              body: {
                title: newWorkspaceTitle(t.workspace.defaultTitlePrefix),
                managed_database_id: selectedDatabase.id,
              },
            },
          );

          nextConversations = [replacementConversation];
          nextActiveConversation = replacementConversation;
        }

        setConversations(nextConversations);
        setActiveConversation(nextActiveConversation);
      } catch (error) {
        toast.error(
          error instanceof Error ? error.message : t.workspace.deleteFailed,
        );
      } finally {
        setIsDeletingWorkspace(false);
      }
    },
    [
      activeConversation,
      conversations,
      isDeletingWorkspace,
      selectedDatabase.id,
      t.workspace.defaultTitlePrefix,
      t.workspace.deleteFailed,
      token,
    ],
  );

  const updatePaneWidth = useCallback((clientX: number) => {
    const workspace = workspaceRef.current;

    if (!workspace) {
      return;
    }

    const rect = workspace.getBoundingClientRect();
    const availableWidth = rect.width;

    if (availableWidth <= MIN_AI_WIDTH + MIN_BI_WIDTH) {
      return;
    }

    const nextLeftWidth = clientX - rect.left;
    const minPercent = (MIN_AI_WIDTH / availableWidth) * 100;
    const maxPercent = ((availableWidth - MIN_BI_WIDTH) / availableWidth) * 100;
    const nextPercent = (nextLeftWidth / availableWidth) * 100;

    setAiWidth(Math.min(maxPercent, Math.max(minPercent, nextPercent)));
  }, []);

  const handleDividerPointerDown = useCallback(
    (event: PointerEvent<HTMLButtonElement>) => {
      event.preventDefault();
      setIsDragging(true);
      updatePaneWidth(event.clientX);
    },
    [updatePaneWidth],
  );

  const handleWorkspacePointerMove = useCallback(
    (event: PointerEvent<HTMLDivElement>) => {
      if (!isDragging) {
        return;
      }

      updatePaneWidth(event.clientX);
    },
    [isDragging, updatePaneWidth],
  );

  const handleWorkspacePointerUp = useCallback(() => {
    setIsDragging(false);
  }, []);

  const handleDividerDoubleClick = useCallback(
    (event: MouseEvent<HTMLButtonElement>) => {
      event.preventDefault();
      setAiWidth(DEFAULT_AI_PERCENT);
    },
    [],
  );

  return (
    <main className="min-h-screen bg-background text-foreground">
      <div className="flex min-h-screen">
        <IconSidebar
          user={user}
          conversations={conversations}
          activeConversationId={activeConversation?.id ?? null}
          isCreatingWorkspace={isCreatingWorkspace}
          isWorkspaceLoading={isWorkspaceLoading}
          onCreateWorkspace={() => void handleCreateWorkspace()}
          onSelectWorkspace={setActiveConversation}
          onDatabaseExit={onDatabaseExit}
        />
        <section
          ref={workspaceRef}
          className={cn(
            "grid min-w-0 flex-1 grid-cols-1 gap-0 bg-muted/30 p-3 lg:h-screen lg:grid-cols-[var(--ai-pane-width)_10px_minmax(0,1fr)] lg:overflow-hidden",
            isDragging && "cursor-col-resize select-none",
          )}
          style={workspaceStyle}
          onPointerMove={handleWorkspacePointerMove}
          onPointerUp={handleWorkspacePointerUp}
          onPointerLeave={handleWorkspacePointerUp}
        >
          {isWorkspaceLoading || !activeConversation ? (
            <WorkspaceLoadingPanel />
          ) : (
            <>
              <ChatPanel
                key={`ai-${selectedDatabase.id}-${activeConversation.id}`}
                token={token}
                selectedDatabase={selectedDatabase}
                conversation={activeConversation}
                isDeletingWorkspace={isDeletingWorkspace}
                onConversationUpdated={handleConversationUpdated}
                onDeleteConversation={handleDeleteWorkspace}
              />
              <SplitHandle
                isDragging={isDragging}
                onPointerDown={handleDividerPointerDown}
                onDoubleClick={handleDividerDoubleClick}
              />
              <BiWorkspacePanel
                key={`bi-${selectedDatabase.id}-${activeConversation.id}`}
                token={token}
                conversationId={activeConversation.id}
                selectedDatabase={selectedDatabase}
              />
            </>
          )}
        </section>
      </div>
    </main>
  );
}

function WorkspaceLoadingPanel() {
  const { t } = useI18n();

  return (
    <section className="col-span-full flex min-h-[calc(100vh-1.5rem)] min-w-0 items-center justify-center rounded-lg border bg-card text-card-foreground shadow-sm lg:h-[calc(100vh-1.5rem)]">
      <div className="flex items-center gap-3 text-sm text-muted-foreground">
        <Loader2 className="size-4 animate-spin" aria-hidden />
        {t.workspace.loadingWorkspace}
      </div>
    </section>
  );
}

function IconSidebar({
  user,
  conversations,
  activeConversationId,
  isCreatingWorkspace,
  isWorkspaceLoading,
  onCreateWorkspace,
  onSelectWorkspace,
  onDatabaseExit,
}: {
  user: PublicUser;
  conversations: ChatConversation[];
  activeConversationId: string | null;
  isCreatingWorkspace: boolean;
  isWorkspaceLoading: boolean;
  onCreateWorkspace: () => void;
  onSelectWorkspace: (conversation: ChatConversation) => void;
  onDatabaseExit: () => void;
}) {
  const { t } = useI18n();
  const initials = user.display_name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part[0])
    .join("")
    .toUpperCase();

  return (
    <aside className="flex w-14 shrink-0 flex-col items-center border-r bg-sidebar text-sidebar-foreground">
      <div className="flex h-16 w-full items-center justify-center border-b border-sidebar-border">
        <div
          className="flex size-9 items-center justify-center rounded-md bg-sidebar-primary text-sidebar-primary-foreground shadow-sm"
          aria-label="Liquid"
          title="Liquid"
        >
          <Sparkles className="size-5" aria-hidden />
        </div>
      </div>
      <nav className="flex flex-1 flex-col items-center gap-2 overflow-y-auto py-4">
        {conversations.map((conversation) => (
          <SidebarIcon
            key={conversation.id}
            icon={<Bot className="size-5" aria-hidden />}
            label={conversation.title}
            active={conversation.id === activeConversationId}
            onClick={() => onSelectWorkspace(conversation)}
          />
        ))}
        <SidebarIcon
          icon={
            isCreatingWorkspace ? (
              <Loader2 className="size-5 animate-spin" aria-hidden />
            ) : (
              <Plus className="size-5" aria-hidden />
            )
          }
          label={t.workspace.newWorkspace}
          active={isCreatingWorkspace}
          disabled={isWorkspaceLoading || isCreatingWorkspace}
          onClick={onCreateWorkspace}
        />
      </nav>
      <div className="flex w-full flex-col items-center gap-2 border-t border-sidebar-border py-3">
        <div
          className="flex size-9 items-center justify-center rounded-md border bg-sidebar-accent text-xs font-semibold text-sidebar-accent-foreground"
          title={user.email}
          aria-label={user.email}
        >
          {initials || user.email.slice(0, 1).toUpperCase()}
        </div>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="size-10 rounded-md text-sidebar-foreground/70 hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
          aria-label={t.workspace.returnToDatabases}
          title={t.workspace.returnToDatabases}
          onClick={onDatabaseExit}
        >
          <LogOut className="size-5" aria-hidden />
        </Button>
      </div>
    </aside>
  );
}

function SidebarIcon({
  icon,
  label,
  active = false,
  disabled = false,
  onClick,
}: {
  icon: ReactNode;
  label: string;
  active?: boolean;
  disabled?: boolean;
  onClick?: MouseEventHandler<HTMLButtonElement>;
}) {
  return (
    <Button
      type="button"
      variant="ghost"
      size="icon"
      disabled={disabled}
      className={cn(
        "size-10 rounded-md text-sidebar-foreground/70 hover:bg-sidebar-accent hover:text-sidebar-accent-foreground",
        active &&
          "bg-sidebar-accent text-sidebar-accent-foreground shadow-xs ring-1 ring-sidebar-border",
      )}
      aria-label={label}
      title={label}
      onClick={onClick}
    >
      {icon}
    </Button>
  );
}

function SplitHandle({
  isDragging,
  onPointerDown,
  onDoubleClick,
}: {
  isDragging: boolean;
  onPointerDown: (event: PointerEvent<HTMLButtonElement>) => void;
  onDoubleClick: (event: MouseEvent<HTMLButtonElement>) => void;
}) {
  const { t } = useI18n();

  return (
    <div className="hidden items-stretch justify-center px-1 lg:flex">
      <button
        type="button"
        className={cn(
          "group my-1 flex w-2 cursor-col-resize items-center justify-center rounded-md outline-none transition-colors hover:bg-accent focus-visible:ring-[3px] focus-visible:ring-ring/50",
          isDragging && "bg-accent",
        )}
        onPointerDown={onPointerDown}
        onDoubleClick={onDoubleClick}
        aria-label={t.workspace.splitHandleLabel}
        title={t.workspace.splitHandleTitle}
      >
        <span className="h-16 w-1 rounded-full bg-border transition-colors group-hover:bg-foreground/40" />
      </button>
    </div>
  );
}
