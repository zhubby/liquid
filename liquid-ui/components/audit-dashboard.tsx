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
import {
  Activity,
  Bot,
  CheckCircle2,
  ChevronDown,
  Database,
  FileText,
  Loader2,
  LogOut,
  PanelsLeftRight,
  Plus,
  Search,
  ShieldCheck,
  Sparkles,
  Table2,
  TrendingUp,
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
import { ChatPanel } from "@/components/chat-workspace";
import {
  type ChatConversation,
  type ManagedDatabase,
  type PublicUser,
  apiRequest,
} from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

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

    const initializeWorkspaces = async () => {
      setIsWorkspaceLoading(true);

      try {
        const existingConversations = await apiRequest<ChatConversation[]>(
          "/api/v1/chat/conversations",
          { token },
        );
        const initialConversation =
          existingConversations[0] ??
          (await apiRequest<ChatConversation>("/api/v1/chat/conversations", {
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
      const nextConversation = await apiRequest<ChatConversation>(
        "/api/v1/chat/conversations",
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
    (conversation: ChatConversation) => {
      setActiveConversation(conversation);
    },
    [],
  );

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
              <ChatPanel
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
