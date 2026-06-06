"use client";

import {
  type CSSProperties,
  type FormEvent,
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
import {
  Activity,
  AlertTriangle,
  Bot,
  CheckCircle2,
  ChevronDown,
  Database,
  FileText,
  Loader2,
  LogOut,
  MessageSquare,
  PanelsLeftRight,
  Plus,
  Search,
  Send,
  ShieldCheck,
  Sparkles,
  Table2,
  TrendingUp,
  Trash2,
  X,
} from "lucide-react";
import { toast } from "sonner";
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
import {
  type AgentAction,
  type AgentConversation,
  type AgentEvent,
  type AgentMessage,
  type AgentTurn,
  type ManagedDatabase,
  type PublicUser,
  type UpdateAgentConversationRequest,
  apiRequest,
  apiStream,
} from "@/lib/api";
import { type Locale, useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

type MessageRole = "assistant" | "user";
type RiskLevel = "low" | "medium" | "high" | "critical";
type DatasetStatus = "normal" | "watch" | "needsAction";
type MetricTone = "primary" | "success" | "warning" | "danger";
type WeekdayKey =
  | "monday"
  | "tuesday"
  | "wednesday"
  | "thursday"
  | "friday"
  | "saturday"
  | "sunday";
type CategoryKey =
  | "customerProfile"
  | "orderFacts"
  | "accessAudit"
  | "marketing"
  | "inventory";
type DatasetOwnerKey =
  | "dataGovernance"
  | "transactionPlatform"
  | "securityPlatform"
  | "growthAnalytics"
  | "supplyChain";
type UpdatedAtKey =
  | "tenMinutes"
  | "eighteenMinutes"
  | "twentyFourMinutes"
  | "fortyTwoMinutes"
  | "oneHour";

type ChatMessage = {
  id: string;
  role: MessageRole;
  content: string;
  time: string;
  metric?: string;
  pending?: boolean;
};

type WorkspaceMetric = {
  label: string;
  value: string;
  change: string;
  tone: MetricTone;
  icon: ReactNode;
};

type TrendPoint = {
  day: WeekdayKey;
  queries: number;
  risks: number;
  passRate: number;
};

type CategoryPoint = {
  key: CategoryKey;
  count: number;
  risk: RiskLevel;
};

type DatasetRow = {
  dataset: string;
  owner: DatasetOwnerKey;
  queries: number;
  risk: RiskLevel;
  status: DatasetStatus;
  updatedAt: UpdatedAtKey;
};

type AuditDashboardProps = {
  token: string;
  user: PublicUser;
  selectedDatabase: ManagedDatabase;
  onDatabaseExit: () => void;
};

const MIN_AI_WIDTH = 320;
const MIN_BI_WIDTH = 520;
const DEFAULT_AI_PERCENT = 38;

const trendData: TrendPoint[] = [
  { day: "monday", queries: 1840, risks: 68, passRate: 96.3 },
  { day: "tuesday", queries: 1935, risks: 71, passRate: 96.5 },
  { day: "wednesday", queries: 2018, risks: 82, passRate: 95.9 },
  { day: "thursday", queries: 1762, risks: 49, passRate: 97.2 },
  { day: "friday", queries: 2114, risks: 76, passRate: 96.4 },
  { day: "saturday", queries: 1588, risks: 44, passRate: 97.1 },
  { day: "sunday", queries: 1589, risks: 48, passRate: 97.0 },
];

const categoryData: CategoryPoint[] = [
  { key: "customerProfile", count: 144, risk: "high" },
  { key: "orderFacts", count: 96, risk: "medium" },
  { key: "accessAudit", count: 31, risk: "critical" },
  { key: "marketing", count: 87, risk: "medium" },
  { key: "inventory", count: 80, risk: "low" },
];

const datasetRows: DatasetRow[] = [
  {
    dataset: "customer_profile",
    owner: "dataGovernance",
    queries: 2846,
    risk: "high",
    status: "needsAction",
    updatedAt: "tenMinutes",
  },
  {
    dataset: "order_fact_daily",
    owner: "transactionPlatform",
    queries: 2318,
    risk: "medium",
    status: "watch",
    updatedAt: "eighteenMinutes",
  },
  {
    dataset: "access_audit_log",
    owner: "securityPlatform",
    queries: 1460,
    risk: "critical",
    status: "needsAction",
    updatedAt: "twentyFourMinutes",
  },
  {
    dataset: "marketing_funnel",
    owner: "growthAnalytics",
    queries: 1952,
    risk: "medium",
    status: "watch",
    updatedAt: "fortyTwoMinutes",
  },
  {
    dataset: "inventory_snapshot",
    owner: "supplyChain",
    queries: 1264,
    risk: "low",
    status: "normal",
    updatedAt: "oneHour",
  },
];

const chartColors = {
  primary: "var(--chart-2)",
  secondary: "var(--chart-3)",
  warning: "var(--chart-4)",
  danger: "var(--destructive)",
  grid: "var(--border)",
  text: "var(--muted-foreground)",
  tooltipBackground: "var(--popover)",
  tooltipBorder: "var(--border)",
  tooltipText: "var(--popover-foreground)",
};

const riskColors: Record<RiskLevel, string> = {
  low: "var(--chart-2)",
  medium: "var(--chart-4)",
  high: "var(--chart-5)",
  critical: "var(--destructive)",
};

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
  const [conversations, setConversations] = useState<AgentConversation[]>([]);
  const [activeConversation, setActiveConversation] =
    useState<AgentConversation | null>(null);
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

    const initializeWorkspaces = async () => {
      setIsWorkspaceLoading(true);

      try {
        const existingConversations = await apiRequest<AgentConversation[]>(
          "/api/v1/agent/conversations",
          { token },
        );
        const initialConversation =
          existingConversations[0] ??
          (await apiRequest<AgentConversation>("/api/v1/agent/conversations", {
            method: "POST",
            token,
            body: { title: newWorkspaceTitle(t.workspace.defaultTitlePrefix) },
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
  }, [t.workspace.defaultTitlePrefix, t.workspace.loadFailed, token]);

  const handleCreateWorkspace = useCallback(async () => {
    if (isCreatingWorkspace) {
      return;
    }

    setIsCreatingWorkspace(true);

    try {
      const nextConversation = await apiRequest<AgentConversation>(
        "/api/v1/agent/conversations",
        {
          method: "POST",
          token,
          body: { title: newWorkspaceTitle(t.workspace.defaultTitlePrefix) },
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
    t.workspace.createFailed,
    t.workspace.defaultTitlePrefix,
    token,
  ]);

  const handleSelectWorkspace = useCallback(
    (conversation: AgentConversation) => {
      setActiveConversation(conversation);
    },
    [],
  );

  const handleConversationUpdated = useCallback(
    (updatedConversation: AgentConversation) => {
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
        await apiRequest<void>(
          `/api/v1/agent/conversations/${conversationId}`,
          {
            method: "DELETE",
            token,
          },
        );

        const remainingConversations = conversations.filter(
          (conversation) => conversation.id !== conversationId,
        );
        let nextConversations = remainingConversations;
        let nextActiveConversation =
          activeConversation?.id === conversationId
            ? remainingConversations[0] ?? null
            : activeConversation;

        if (nextConversations.length === 0) {
          const replacementConversation = await apiRequest<AgentConversation>(
            "/api/v1/agent/conversations",
            {
              method: "POST",
              token,
              body: { title: newWorkspaceTitle(t.workspace.defaultTitlePrefix) },
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
          onSelectWorkspace={handleSelectWorkspace}
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
              <AiPanel
                key={`ai-${activeConversation.id}`}
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
              <BiPanel
                key={`bi-${activeConversation.id}`}
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
  conversations: AgentConversation[];
  activeConversationId: string | null;
  isCreatingWorkspace: boolean;
  isWorkspaceLoading: boolean;
  onCreateWorkspace: () => void;
  onSelectWorkspace: (conversation: AgentConversation) => void;
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

function AiPanel({
  token,
  selectedDatabase,
  conversation,
  isDeletingWorkspace,
  onConversationUpdated,
  onDeleteConversation,
}: {
  token: string;
  selectedDatabase: ManagedDatabase;
  conversation: AgentConversation;
  isDeletingWorkspace: boolean;
  onConversationUpdated: (conversation: AgentConversation) => void;
  onDeleteConversation: (conversationId: string) => void | Promise<void>;
}) {
  const { locale, t } = useI18n();
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [actions, setActions] = useState<AgentAction[]>([]);
  const [titleInput, setTitleInput] = useState(conversation.title);
  const [isSavingTitle, setIsSavingTitle] = useState(false);
  const [isDeleteDialogOpen, setIsDeleteDialogOpen] = useState(false);
  const [input, setInput] = useState("");
  const [isLoading, setIsLoading] = useState(true);
  const [isSending, setIsSending] = useState(false);
  const activeStreamRef = useRef<AbortController | null>(null);
  const activeConversationIdRef = useRef(conversation.id);
  const activeSendRef = useRef<string | null>(null);
  const loadVersionRef = useRef(0);

  const loadMessagesAndActions = useCallback(
    async (conversationId: string) => {
      const [nextMessages, nextActions] = await Promise.all([
        apiRequest<AgentMessage[]>(
          `/api/v1/agent/conversations/${conversationId}/messages`,
          { token },
        ),
        apiRequest<AgentAction[]>(
          `/api/v1/agent/actions?conversation_id=${conversationId}`,
          { token },
        ),
      ]);

      return {
        actions: nextActions,
        messages: nextMessages.map((message) =>
          chatMessageFromAgentMessage(message, locale),
        ),
      };
    },
    [locale, token],
  );

  useEffect(() => {
    setTitleInput(conversation.title);
  }, [conversation.id, conversation.title]);

  useEffect(() => {
    let cancelled = false;
    const loadVersion = loadVersionRef.current + 1;

    loadVersionRef.current = loadVersion;
    activeConversationIdRef.current = conversation.id;
    activeSendRef.current = null;
    activeStreamRef.current?.abort();
    activeStreamRef.current = null;
    setInput("");
    setIsSending(false);
    setMessages([]);
    setActions([]);
    setIsLoading(true);

    const loadConversationState = async () => {
      try {
        const nextState = await loadMessagesAndActions(conversation.id);

        if (cancelled || loadVersionRef.current !== loadVersion) {
          return;
        }

        setMessages(nextState.messages);
        setActions(nextState.actions);
      } catch (error) {
        if (!cancelled && loadVersionRef.current === loadVersion) {
          toast.error(
            error instanceof Error ? error.message : t.workspace.agentLoadFailed,
          );
        }
      } finally {
        if (!cancelled && loadVersionRef.current === loadVersion) {
          setIsLoading(false);
        }
      }
    };

    void loadConversationState();

    return () => {
      cancelled = true;
      activeSendRef.current = null;
      activeStreamRef.current?.abort();
      activeStreamRef.current = null;
    };
  }, [conversation.id, loadMessagesAndActions, t.workspace.agentLoadFailed]);

  const mergeAction = useCallback((action: AgentAction) => {
    setActions((current) => {
      if (current.some((item) => item.id === action.id)) {
        return current.map((item) => (item.id === action.id ? action : item));
      }

      return [action, ...current];
    });
  }, []);

  const commitWorkspaceTitle = useCallback(async () => {
    const title = titleInput.trim();

    if (!title) {
      setTitleInput(conversation.title);
      return;
    }

    if (title === conversation.title || isSavingTitle) {
      return;
    }

    setIsSavingTitle(true);

    try {
      const body: UpdateAgentConversationRequest = { title };
      const updatedConversation = await apiRequest<AgentConversation>(
        `/api/v1/agent/conversations/${conversation.id}`,
        {
          method: "PATCH",
          token,
          body,
        },
      );

      setTitleInput(updatedConversation.title);
      onConversationUpdated(updatedConversation);
    } catch (error) {
      setTitleInput(conversation.title);
      toast.error(
        error instanceof Error ? error.message : t.workspace.renameFailed,
      );
    } finally {
      setIsSavingTitle(false);
    }
  }, [
    conversation.id,
    conversation.title,
    isSavingTitle,
    onConversationUpdated,
    t.workspace.renameFailed,
    titleInput,
    token,
  ]);

  const submitPrompt = useCallback(
    async (prompt?: string) => {
      const content = (prompt ?? input).trim();

      if (!content || isSending || activeSendRef.current) {
        return;
      }

      const conversationId = conversation.id;
      setInput("");
      setIsSending(true);
      activeStreamRef.current?.abort();

      const localUserMessage: ChatMessage = {
        id: `local-user-${Date.now()}`,
        role: "user",
        content,
        time: nowTimeLabel(locale),
        pending: true,
      };
      const sendKey = `${conversationId}:${localUserMessage.id}`;

      activeSendRef.current = sendKey;
      setMessages((current) => [...current, localUserMessage]);

      try {
        const turn = await apiRequest<AgentTurn>(
          `/api/v1/agent/conversations/${conversationId}/turns`,
          {
            method: "POST",
            token,
            body: {
              message: content,
              managed_database_id: selectedDatabase.id,
              dashboard_context: {
                active_view: "ai",
                date_range: "last_7_days",
              },
              client_request_id: localUserMessage.id,
            },
          },
        );
        const controller = new AbortController();
        activeStreamRef.current = controller;

        await apiStream<AgentEvent>(
          `/api/v1/agent/turns/${turn.id}/events?after_seq=0`,
          {
            token,
            signal: controller.signal,
            onEvent: (event) => {
              if (activeConversationIdRef.current !== conversationId) {
                return;
              }

              if (event.type === "assistant_delta") {
                const assistantContent = eventPayloadString(
                  event.payload,
                  "content",
                );

                if (assistantContent) {
                  setMessages((current) =>
                    upsertStreamingAssistantMessage(
                      current,
                      turn.id,
                      assistantContent,
                      locale,
                    ),
                  );
                }
              }

              if (event.type === "action_proposed") {
                const action = eventPayloadAction(event.payload);

                if (action) {
                  mergeAction(action);
                }
              }
            },
          },
        );

        const nextState = await loadMessagesAndActions(conversationId);

        if (
          activeConversationIdRef.current === conversationId &&
          activeSendRef.current === sendKey
        ) {
          setMessages(nextState.messages);
          setActions(nextState.actions);
        }
      } catch (error) {
        if (
          !(error instanceof DOMException && error.name === "AbortError") &&
          activeConversationIdRef.current === conversationId
        ) {
          toast.error(error instanceof Error ? error.message : t.workspace.sendFailed);
        }
      } finally {
        if (activeSendRef.current === sendKey) {
          activeSendRef.current = null;
          activeStreamRef.current = null;
          setIsSending(false);
        }
      }
    },
    [
      conversation.id,
      input,
      isSending,
      loadMessagesAndActions,
      locale,
      mergeAction,
      selectedDatabase.id,
      t.workspace.sendFailed,
      token,
    ],
  );

  const handleSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    void submitPrompt();
  };

  const handleActionDecision = async (
    action: AgentAction,
    decision: "apply" | "reject",
  ) => {
    try {
      const updated = await apiRequest<AgentAction>(
        `/api/v1/agent/actions/${action.id}/${decision}`,
        {
          method: "POST",
          token,
          body: {},
        },
      );
      mergeAction(updated);
    } catch (error) {
      toast.error(error instanceof Error ? error.message : t.workspace.actionFailed);
    }
  };

  const visibleMessages =
    messages.length > 0
      ? messages
      : [
          {
            id: "intro",
            role: "assistant" as const,
            content: t.workspace.introMessage(selectedDatabase.name),
            time: nowTimeLabel(locale),
          },
        ];
  const proposedActions = actions.filter((action) => action.status === "proposed");

  return (
    <section className="flex min-h-[calc(100vh-1.5rem)] min-w-0 flex-col rounded-lg border bg-card text-card-foreground shadow-sm lg:h-[calc(100vh-1.5rem)]">
      <header className="flex shrink-0 items-center justify-between gap-3 border-b px-4 py-3">
        <div className="min-w-0 flex-1">
          <div className="flex min-w-0 items-center gap-2">
            <label
              className="sr-only"
              htmlFor={`workspace-title-${conversation.id}`}
            >
              {t.workspace.workspaceName}
            </label>
            <input
              id={`workspace-title-${conversation.id}`}
              className="min-w-0 flex-1 truncate rounded-sm bg-transparent text-base font-semibold outline-none transition-colors hover:bg-muted/50 focus-visible:bg-background focus-visible:ring-[3px] focus-visible:ring-ring/50 disabled:opacity-70"
              value={titleInput}
              disabled={isSavingTitle}
              onChange={(event) => setTitleInput(event.target.value)}
              onBlur={() => void commitWorkspaceTitle()}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  event.currentTarget.blur();
                }

                if (event.key === "Escape") {
                  setTitleInput(conversation.title);
                  event.currentTarget.blur();
                }
              }}
            />
          </div>
          <div className="mt-1 flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground">
            <Database className="size-3.5 shrink-0" aria-hidden />
            <span className="truncate">
              {selectedDatabase.name} / {selectedDatabase.database}
            </span>
          </div>
        </div>
        <Button
          type="button"
          variant="outline"
          size="icon"
          className="size-9 shrink-0 rounded-md text-destructive hover:bg-destructive/10 hover:text-destructive"
          aria-label={t.workspace.deleteWorkspaceLabel(conversation.title)}
          title={t.workspace.deleteWorkspaceTitle}
          disabled={isDeletingWorkspace}
          onClick={() => setIsDeleteDialogOpen(true)}
        >
          {isDeletingWorkspace ? (
            <Loader2 className="size-4 animate-spin" aria-hidden />
          ) : (
            <Trash2 className="size-4" aria-hidden />
          )}
        </Button>
      </header>

      {isDeleteDialogOpen ? (
        <ConfirmDeleteWorkspaceDialog
          conversationTitle={conversation.title}
          isDeleting={isDeletingWorkspace}
          onCancel={() => setIsDeleteDialogOpen(false)}
          onConfirm={() => {
            void onDeleteConversation(conversation.id);
          }}
        />
      ) : null}

      <div className="flex min-h-0 flex-1 flex-col">
        <div className="flex-1 space-y-4 overflow-y-auto px-4 py-4">
          {isLoading ? (
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <Loader2 className="size-4 animate-spin" aria-hidden />
              {t.workspace.loadingConversation}
            </div>
          ) : null}
          {visibleMessages.map((message) => (
            <ChatBubble key={message.id} message={message} />
          ))}
          {proposedActions.length > 0 ? (
            <div className="space-y-2">
              {proposedActions.map((action) => (
                <AgentActionCard
                  key={action.id}
                  action={action}
                  onApply={() => void handleActionDecision(action, "apply")}
                  onReject={() => void handleActionDecision(action, "reject")}
                />
              ))}
            </div>
          ) : null}
        </div>

        <div className="border-t bg-card px-4 py-3">
          <div className="mb-3 flex flex-wrap gap-2">
            {t.workspace.quickPrompts.map((prompt) => (
              <Button
                key={prompt}
                type="button"
                variant="outline"
                size="sm"
                className="h-8 rounded-md px-2.5 text-xs"
                disabled={isSending || isLoading}
                onClick={() => void submitPrompt(prompt)}
              >
                {prompt}
              </Button>
            ))}
          </div>
          <form
            className="flex items-end gap-2 rounded-lg border bg-background p-2 shadow-xs"
            onSubmit={handleSubmit}
          >
            <label className="sr-only" htmlFor="ai-message">
              {t.workspace.inputLabel}
            </label>
            <textarea
              id="ai-message"
              className="max-h-32 min-h-16 flex-1 resize-none bg-transparent px-2 py-1.5 text-sm outline-none placeholder:text-muted-foreground"
              placeholder={t.workspace.inputPlaceholder}
              value={input}
              onChange={(event) => setInput(event.target.value)}
              disabled={isSending || isLoading}
            />
            <Button
              type="submit"
              size="icon"
              aria-label={t.workspace.sendQuestion}
              title={t.workspace.send}
              disabled={isSending || isLoading || !input.trim()}
            >
              {isSending ? (
                <Loader2 className="size-4 animate-spin" aria-hidden />
              ) : (
                <Send className="size-4" aria-hidden />
              )}
            </Button>
          </form>
        </div>
      </div>
    </section>
  );
}

function ConfirmDeleteWorkspaceDialog({
  conversationTitle,
  isDeleting,
  onCancel,
  onConfirm,
}: {
  conversationTitle: string;
  isDeleting: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const { t } = useI18n();

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-background/70 p-4"
      role="dialog"
      aria-modal="true"
      aria-labelledby="delete-workspace-title"
    >
      <div className="w-full max-w-sm rounded-lg border bg-card p-4 text-card-foreground shadow-lg">
        <div className="flex items-start gap-3">
          <div className="flex size-9 shrink-0 items-center justify-center rounded-md border border-destructive/25 bg-destructive/10 text-destructive">
            <AlertTriangle className="size-4" aria-hidden />
          </div>
          <div className="min-w-0">
            <h2 id="delete-workspace-title" className="text-sm font-semibold">
              {t.workspace.deleteWorkspaceTitle}
            </h2>
            <p className="mt-1 text-sm leading-6 text-muted-foreground">
              {t.workspace.deleteConfirm(conversationTitle)}
            </p>
          </div>
        </div>
        <div className="mt-4 flex justify-end gap-2">
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={isDeleting}
            onClick={onCancel}
          >
            {t.common.cancel}
          </Button>
          <Button
            type="button"
            variant="destructive"
            size="sm"
            disabled={isDeleting}
            onClick={onConfirm}
          >
            {isDeleting ? (
              <Loader2 className="size-4 animate-spin" aria-hidden />
            ) : (
              <Trash2 className="size-4" aria-hidden />
            )}
            {t.common.delete}
          </Button>
        </div>
      </div>
    </div>
  );
}

function ChatBubble({ message }: { message: ChatMessage }) {
  const { t } = useI18n();
  const isUser = message.role === "user";

  return (
    <article
      className={cn("flex gap-3", isUser && "flex-row-reverse text-right")}
    >
      <div
        className={cn(
          "mt-1 flex size-8 shrink-0 items-center justify-center rounded-md border",
          isUser
            ? "bg-primary text-primary-foreground"
            : "bg-secondary text-secondary-foreground",
        )}
      >
        {isUser ? (
          <MessageSquare className="size-4" aria-hidden />
        ) : (
          <Bot className="size-4" aria-hidden />
        )}
      </div>
      <div className={cn("min-w-0 max-w-[86%]", isUser && "items-end")}>
        <div
          className={cn(
            "rounded-lg border px-3 py-2 text-sm leading-6 shadow-xs",
            isUser
              ? "bg-primary text-primary-foreground"
              : "bg-background text-foreground",
          )}
        >
          {message.content}
        </div>
        <div
          className={cn(
            "mt-1 flex items-center gap-2 text-xs text-muted-foreground",
            isUser && "justify-end",
          )}
        >
          <span>{message.time}</span>
          {message.metric ? (
            <span className="rounded-sm bg-secondary px-1.5 py-0.5 text-secondary-foreground">
              {message.metric}
            </span>
          ) : null}
          {message.pending ? <span>{t.workspace.pending}</span> : null}
        </div>
      </div>
    </article>
  );
}

function AgentActionCard({
  action,
  onApply,
  onReject,
}: {
  action: AgentAction;
  onApply: () => void;
  onReject: () => void;
}) {
  const { t } = useI18n();

  return (
    <article className="rounded-lg border bg-background p-3 shadow-xs">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <h3 className="text-sm font-medium">{action.title}</h3>
            <Badge variant="outline" className="h-6 rounded-md">
              {t.workspace.actionLabels[action.kind]}
            </Badge>
          </div>
          <p className="mt-1 text-xs leading-5 text-muted-foreground">
            {action.description}
          </p>
        </div>
        <Badge variant="secondary" className="rounded-md">
          {t.workspace.actionStatuses[action.status]}
        </Badge>
      </div>
      <div className="mt-3 flex flex-wrap gap-2">
        <Button type="button" size="sm" onClick={onApply}>
          <CheckCircle2 className="size-4" aria-hidden />
          {t.workspace.confirm}
        </Button>
        <Button type="button" variant="outline" size="sm" onClick={onReject}>
          <X className="size-4" aria-hidden />
          {t.workspace.reject}
        </Button>
      </div>
    </article>
  );
}

function chatMessageFromAgentMessage(
  message: AgentMessage,
  locale: Locale,
): ChatMessage {
  return {
    id: message.id,
    role: message.role === "user" ? "user" : "assistant",
    content: message.content,
    time: timeLabel(message.created_at, locale),
  };
}

function upsertStreamingAssistantMessage(
  messages: ChatMessage[],
  turnId: string,
  content: string,
  locale: Locale,
): ChatMessage[] {
  const id = `stream-${turnId}`;

  if (messages.some((message) => message.id === id)) {
    return messages.map((message) =>
      message.id === id
        ? {
            ...message,
            content,
          }
        : message,
    );
  }

  return [
    ...messages,
    {
      id,
      role: "assistant",
      content,
      time: nowTimeLabel(locale),
      pending: true,
    },
  ];
}

function eventPayloadString(payload: unknown, key: string): string | null {
  if (!payload || typeof payload !== "object") {
    return null;
  }

  const value = (payload as Record<string, unknown>)[key];

  return typeof value === "string" ? value : null;
}

function eventPayloadAction(payload: unknown): AgentAction | null {
  if (!payload || typeof payload !== "object") {
    return null;
  }

  const value = (payload as Record<string, unknown>).action;

  if (!value || typeof value !== "object") {
    return null;
  }

  return value as AgentAction;
}

function timeLabel(value: string, locale: Locale): string {
  return new Date(value).toLocaleTimeString(locale, {
    hour: "2-digit",
    minute: "2-digit",
  });
}

function nowTimeLabel(locale: Locale): string {
  return timeLabel(new Date().toISOString(), locale);
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

function BiPanel({ selectedDatabase }: { selectedDatabase: ManagedDatabase }) {
  const { t } = useI18n();
  const metrics = useMemo<WorkspaceMetric[]>(
    () => [
      {
        label: t.dashboard.metrics.totalQueries,
        value: "12,846",
        change: "+8.2%",
        tone: "primary",
        icon: <Database className="size-4" aria-hidden />,
      },
      {
        label: t.dashboard.metrics.passRate,
        value: "96.8%",
        change: "+1.1%",
        tone: "success",
        icon: <CheckCircle2 className="size-4" aria-hidden />,
      },
      {
        label: t.dashboard.metrics.riskEvents,
        value: "438",
        change: "-4.5%",
        tone: "warning",
        icon: <ShieldCheck className="size-4" aria-hidden />,
      },
      {
        label: t.dashboard.metrics.pendingReview,
        value: "37",
        change: t.dashboard.metrics.highRisk,
        tone: "danger",
        icon: <Activity className="size-4" aria-hidden />,
      },
    ],
    [t],
  );

  return (
    <section className="mt-3 flex min-h-[calc(100vh-1.5rem)] min-w-0 flex-col overflow-hidden rounded-lg border bg-card text-card-foreground shadow-sm lg:mt-0 lg:h-[calc(100vh-1.5rem)]">
      <header className="flex shrink-0 flex-col gap-3 border-b px-4 py-3 xl:flex-row xl:items-center xl:justify-between">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <h2 className="truncate text-base font-semibold">
              {t.dashboard.title}
            </h2>
            <Badge variant="outline" className="h-6 rounded-md">
              {selectedDatabase.name}
            </Badge>
          </div>
          <p className="mt-1 truncate text-xs text-muted-foreground">
            {selectedDatabase.host}:{selectedDatabase.port} /{" "}
            {selectedDatabase.database}
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button type="button" variant="outline" size="sm">
            <Search className="size-4" aria-hidden />
            {t.common.search}
          </Button>
          <Button type="button" variant="outline" size="sm">
            <FileText className="size-4" aria-hidden />
            {t.dashboard.export}
          </Button>
          <Button type="button" variant="secondary" size="sm">
            <ChevronDown className="size-4" aria-hidden />
            {t.dashboard.allDatasets}
          </Button>
        </div>
      </header>

      <div className="min-h-0 flex-1 overflow-y-auto px-4 py-4">
        <div className="grid gap-3 md:grid-cols-2 2xl:grid-cols-4">
          {metrics.map((metric) => (
            <MetricCard key={metric.label} metric={metric} />
          ))}
        </div>

        <div className="mt-4 grid gap-4 xl:grid-cols-[1.45fr_1fr]">
          <TrendChart />
          <CategoryChart />
        </div>

        <DatasetTable />
      </div>
    </section>
  );
}

function MetricCard({ metric }: { metric: WorkspaceMetric }) {
  const toneClasses = {
    primary: "border-primary/20 bg-primary/10 text-primary",
    success: "border-chart-2/30 bg-chart-2/15 text-foreground",
    warning: "border-chart-4/30 bg-chart-4/15 text-foreground",
    danger: "border-destructive/25 bg-destructive/10 text-destructive",
  }[metric.tone];

  return (
    <Card className="gap-3 rounded-lg py-4 shadow-xs">
      <CardHeader className="flex flex-row items-center justify-between gap-3 px-4">
        <CardTitle className="text-xs font-medium text-muted-foreground">
          {metric.label}
        </CardTitle>
        <div
          className={cn(
            "flex size-8 shrink-0 items-center justify-center rounded-md border",
            toneClasses,
          )}
        >
          {metric.icon}
        </div>
      </CardHeader>
      <CardContent className="px-4">
        <div className="text-2xl font-semibold tracking-normal">
          {metric.value}
        </div>
        <div className="mt-1 text-xs text-muted-foreground">{metric.change}</div>
      </CardContent>
    </Card>
  );
}

function TrendChart() {
  const { t } = useI18n();
  const localizedTrendData = useMemo(
    () =>
      trendData.map((point) => ({
        ...point,
        dayLabel: t.dashboard.weekdays[point.day],
      })),
    [t],
  );

  return (
    <Card className="rounded-lg py-4 shadow-xs">
      <CardHeader className="flex flex-row items-center justify-between px-4">
        <div>
          <CardTitle className="text-sm">{t.dashboard.trendTitle}</CardTitle>
          <p className="mt-1 text-xs text-muted-foreground">
            {t.dashboard.trendDescription}
          </p>
        </div>
        <PanelsLeftRight className="size-4 text-muted-foreground" aria-hidden />
      </CardHeader>
      <CardContent className="px-2 pt-2 sm:px-4">
        <div className="h-72">
          <ResponsiveContainer width="100%" height="100%">
            <LineChart data={localizedTrendData}>
              <CartesianGrid stroke={chartColors.grid} vertical={false} />
              <XAxis
                dataKey="dayLabel"
                tickLine={false}
                axisLine={false}
                tick={{ fill: chartColors.text, fontSize: 12 }}
              />
              <YAxis
                yAxisId="volume"
                tickLine={false}
                axisLine={false}
                tick={{ fill: chartColors.text, fontSize: 12 }}
                width={44}
              />
              <YAxis
                yAxisId="rate"
                orientation="right"
                domain={[92, 100]}
                tickLine={false}
                axisLine={false}
                tick={{ fill: chartColors.text, fontSize: 12 }}
                width={38}
              />
              <Tooltip content={<ChartTooltip />} />
              <Line
                yAxisId="volume"
                type="monotone"
                dataKey="queries"
                name={t.dashboard.chartKeys.queries}
                stroke={chartColors.primary}
                strokeWidth={2.5}
                dot={false}
              />
              <Line
                yAxisId="volume"
                type="monotone"
                dataKey="risks"
                name={t.dashboard.chartKeys.risks}
                stroke={chartColors.danger}
                strokeWidth={2.5}
                dot={false}
              />
              <Line
                yAxisId="rate"
                type="monotone"
                dataKey="passRate"
                name={t.dashboard.chartKeys.passRate}
                stroke={chartColors.warning}
                strokeWidth={2}
                dot={false}
              />
            </LineChart>
          </ResponsiveContainer>
        </div>
      </CardContent>
    </Card>
  );
}

function CategoryChart() {
  const { t } = useI18n();
  const localizedCategoryData = useMemo(
    () =>
      categoryData.map((entry) => ({
        ...entry,
        name: t.dashboard.categories[entry.key],
      })),
    [t],
  );

  return (
    <Card className="rounded-lg py-4 shadow-xs">
      <CardHeader className="flex flex-row items-center justify-between px-4">
        <div>
          <CardTitle className="text-sm">
            {t.dashboard.riskDistributionTitle}
          </CardTitle>
          <p className="mt-1 text-xs text-muted-foreground">
            {t.dashboard.riskDistributionDescription}
          </p>
        </div>
        <TrendingUp className="size-4 text-muted-foreground" aria-hidden />
      </CardHeader>
      <CardContent className="px-2 pt-2 sm:px-4">
        <div className="h-72">
          <ResponsiveContainer width="100%" height="100%">
            <BarChart data={localizedCategoryData} layout="vertical">
              <CartesianGrid stroke={chartColors.grid} horizontal={false} />
              <XAxis
                type="number"
                tickLine={false}
                axisLine={false}
                tick={{ fill: chartColors.text, fontSize: 12 }}
              />
              <YAxis
                dataKey="name"
                type="category"
                width={76}
                tickLine={false}
                axisLine={false}
                tick={{ fill: chartColors.text, fontSize: 12 }}
              />
              <Tooltip content={<ChartTooltip />} />
              <Bar
                dataKey="count"
                name={t.dashboard.chartKeys.count}
                radius={[0, 6, 6, 0]}
              >
                {localizedCategoryData.map((entry) => (
                  <Cell key={entry.key} fill={riskColors[entry.risk]} />
                ))}
              </Bar>
            </BarChart>
          </ResponsiveContainer>
        </div>
      </CardContent>
    </Card>
  );
}

function DatasetTable() {
  const { locale, t } = useI18n();
  const localizedRows = useMemo(
    () =>
      datasetRows.map((row) => ({
        ...row,
        ownerLabel: t.dashboard.owners[row.owner],
        updatedAtLabel: t.dashboard.updatedAt[row.updatedAt],
      })),
    [t],
  );

  return (
    <Card className="mt-4 rounded-lg py-4 shadow-xs">
      <CardHeader className="flex flex-row items-center justify-between gap-3 px-4">
        <div>
          <CardTitle className="text-sm">
            {t.dashboard.datasetTableTitle}
          </CardTitle>
          <p className="mt-1 text-xs text-muted-foreground">
            {t.dashboard.datasetTableDescription}
          </p>
        </div>
        <Table2 className="size-4 shrink-0 text-muted-foreground" aria-hidden />
      </CardHeader>
      <CardContent className="px-0">
        <div className="overflow-x-auto">
          <table className="w-full min-w-[720px] border-collapse text-sm">
            <thead>
              <tr className="border-y bg-muted/60 text-left text-xs text-muted-foreground">
                <th className="px-4 py-2.5 font-medium">
                  {t.dashboard.tableHeaders.dataset}
                </th>
                <th className="px-4 py-2.5 font-medium">
                  {t.dashboard.tableHeaders.owner}
                </th>
                <th className="px-4 py-2.5 text-right font-medium">
                  {t.dashboard.tableHeaders.queries}
                </th>
                <th className="px-4 py-2.5 font-medium">
                  {t.dashboard.tableHeaders.risk}
                </th>
                <th className="px-4 py-2.5 font-medium">
                  {t.dashboard.tableHeaders.status}
                </th>
                <th className="px-4 py-2.5 font-medium">
                  {t.dashboard.tableHeaders.updatedAt}
                </th>
              </tr>
            </thead>
            <tbody>
              {localizedRows.map((row) => (
                <tr
                  key={row.dataset}
                  className="border-b transition-colors hover:bg-muted/40"
                >
                  <td className="px-4 py-3 font-medium">{row.dataset}</td>
                  <td className="px-4 py-3 text-muted-foreground">
                    {row.ownerLabel}
                  </td>
                  <td className="px-4 py-3 text-right tabular-nums">
                    {row.queries.toLocaleString(locale)}
                  </td>
                  <td className="px-4 py-3">
                    <RiskBadge risk={row.risk} />
                  </td>
                  <td className="px-4 py-3">
                    <StatusBadge status={row.status} />
                  </td>
                  <td className="px-4 py-3 text-muted-foreground">
                    {row.updatedAtLabel}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </CardContent>
    </Card>
  );
}

function RiskBadge({ risk }: { risk: RiskLevel }) {
  const { t } = useI18n();
  const variant = risk === "critical" || risk === "high" ? "destructive" : "outline";

  return (
    <Badge variant={variant} className="rounded-md">
      {t.dashboard.riskLevels[risk]}
    </Badge>
  );
}

function StatusBadge({ status }: { status: DatasetStatus }) {
  const { t } = useI18n();
  const statusClasses = {
    normal: "border-chart-2/30 bg-chart-2/10 text-foreground",
    watch: "border-chart-4/30 bg-chart-4/15 text-foreground",
    needsAction: "border-destructive/25 bg-destructive/10 text-destructive",
  }[status];

  return (
    <span
      className={cn(
        "inline-flex h-6 items-center rounded-md border px-2 text-xs font-medium",
        statusClasses,
      )}
    >
      {t.dashboard.datasetStatuses[status]}
    </span>
  );
}

function ChartTooltip({
  active,
  payload,
  label,
}: {
  active?: boolean;
  payload?: Array<{
    name?: string;
    value?: number | string;
    color?: string;
  }>;
  label?: string;
}) {
  if (!active || !payload?.length) {
    return null;
  }

  return (
    <div
      className="rounded-md border px-3 py-2 text-xs shadow-sm"
      style={{
        background: chartColors.tooltipBackground,
        borderColor: chartColors.tooltipBorder,
        color: chartColors.tooltipText,
      }}
    >
      <div className="mb-1 font-medium">{label}</div>
      <div className="space-y-1">
        {payload.map((entry) => (
          <div key={`${entry.name}-${entry.value}`} className="flex gap-2">
            <span style={{ color: entry.color }}>●</span>
            <span className="text-muted-foreground">{entry.name}</span>
            <span className="font-medium">{entry.value}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
