"use client";

import {
  type CSSProperties,
  type MouseEvent,
  type PointerEvent,
  type ReactNode,
  useCallback,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  Activity,
  BarChart3,
  Bot,
  CheckCircle2,
  ChevronDown,
  Database,
  FileText,
  MessageSquare,
  PanelsLeftRight,
  Search,
  Send,
  ShieldCheck,
  Sparkles,
  Table2,
  TrendingUp,
} from "lucide-react";
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
import { cn } from "@/lib/utils";

type MessageRole = "assistant" | "user";
type RiskLevel = "低" | "中" | "高" | "严重";
type DatasetStatus = "正常" | "观察" | "需处理";
type MetricTone = "primary" | "success" | "warning" | "danger";

type ChatMessage = {
  id: number;
  role: MessageRole;
  content: string;
  time: string;
  metric?: string;
};

type WorkspaceMetric = {
  label: string;
  value: string;
  change: string;
  tone: MetricTone;
  icon: ReactNode;
};

type TrendPoint = {
  day: string;
  查询量: number;
  风险量: number;
  通过率: number;
};

type CategoryPoint = {
  name: string;
  count: number;
  risk: RiskLevel;
};

type DatasetRow = {
  dataset: string;
  owner: string;
  queries: number;
  risk: RiskLevel;
  status: DatasetStatus;
  updatedAt: string;
};

const MIN_AI_WIDTH = 320;
const MIN_BI_WIDTH = 520;
const DEFAULT_AI_PERCENT = 38;

const chatMessages: ChatMessage[] = [
  {
    id: 1,
    role: "assistant",
    content:
      "早上好。我已经汇总最近 7 天的数据访问情况，整体通过率 96.8%，但客户明细表的敏感字段访问明显升高。",
    time: "09:18",
    metric: "96.8% 通过率",
  },
  {
    id: 2,
    role: "user",
    content: "帮我看一下高风险查询主要集中在哪些数据集？",
    time: "09:20",
  },
  {
    id: 3,
    role: "assistant",
    content:
      "高风险主要来自客户画像、订单流水和权限审计三个数据集。建议优先检查无筛选条件的导出查询，以及跨库关联场景。",
    time: "09:21",
    metric: "37 条高风险",
  },
  {
    id: 4,
    role: "user",
    content: "生成一个今天的数据巡检摘要。",
    time: "09:24",
  },
  {
    id: 5,
    role: "assistant",
    content:
      "今日查询量保持稳定，营销分析看板调用增长 12.4%。目前只有 3 个任务需要人工复核，我已在右侧表格按风险优先级排序。",
    time: "09:25",
    metric: "3 项待复核",
  },
];

const quickPrompts = [
  "解释风险上升原因",
  "生成周报摘要",
  "找出异常数据集",
  "给出治理建议",
];

const metrics: WorkspaceMetric[] = [
  {
    label: "查询总量",
    value: "12,846",
    change: "+8.2%",
    tone: "primary",
    icon: <Database className="size-4" aria-hidden />,
  },
  {
    label: "通过率",
    value: "96.8%",
    change: "+1.1%",
    tone: "success",
    icon: <CheckCircle2 className="size-4" aria-hidden />,
  },
  {
    label: "风险事件",
    value: "438",
    change: "-4.5%",
    tone: "warning",
    icon: <ShieldCheck className="size-4" aria-hidden />,
  },
  {
    label: "待复核",
    value: "37",
    change: "高风险",
    tone: "danger",
    icon: <Activity className="size-4" aria-hidden />,
  },
];

const trendData: TrendPoint[] = [
  { day: "周一", 查询量: 1840, 风险量: 68, 通过率: 96.3 },
  { day: "周二", 查询量: 1935, 风险量: 71, 通过率: 96.5 },
  { day: "周三", 查询量: 2018, 风险量: 82, 通过率: 95.9 },
  { day: "周四", 查询量: 1762, 风险量: 49, 通过率: 97.2 },
  { day: "周五", 查询量: 2114, 风险量: 76, 通过率: 96.4 },
  { day: "周六", 查询量: 1588, 风险量: 44, 通过率: 97.1 },
  { day: "周日", 查询量: 1589, 风险量: 48, 通过率: 97.0 },
];

const categoryData: CategoryPoint[] = [
  { name: "客户画像", count: 144, risk: "高" },
  { name: "订单流水", count: 96, risk: "中" },
  { name: "权限审计", count: 31, risk: "严重" },
  { name: "营销分析", count: 87, risk: "中" },
  { name: "库存同步", count: 80, risk: "低" },
];

const datasetRows: DatasetRow[] = [
  {
    dataset: "customer_profile",
    owner: "数据治理组",
    queries: 2846,
    risk: "高",
    status: "需处理",
    updatedAt: "10 分钟前",
  },
  {
    dataset: "order_fact_daily",
    owner: "交易平台",
    queries: 2318,
    risk: "中",
    status: "观察",
    updatedAt: "18 分钟前",
  },
  {
    dataset: "access_audit_log",
    owner: "安全平台",
    queries: 1460,
    risk: "严重",
    status: "需处理",
    updatedAt: "24 分钟前",
  },
  {
    dataset: "marketing_funnel",
    owner: "增长分析",
    queries: 1952,
    risk: "中",
    status: "观察",
    updatedAt: "42 分钟前",
  },
  {
    dataset: "inventory_snapshot",
    owner: "供应链",
    queries: 1264,
    risk: "低",
    status: "正常",
    updatedAt: "1 小时前",
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
  低: "var(--chart-2)",
  中: "var(--chart-4)",
  高: "var(--chart-5)",
  严重: "var(--destructive)",
};

export function AuditDashboard() {
  const workspaceRef = useRef<HTMLDivElement>(null);
  const [aiWidth, setAiWidth] = useState(DEFAULT_AI_PERCENT);
  const [isDragging, setIsDragging] = useState(false);

  const workspaceStyle = useMemo(
    () =>
      ({
        "--ai-pane-width": `${aiWidth}%`,
      }) as CSSProperties,
    [aiWidth],
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
        <IconSidebar />
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
          <AiPanel />
          <SplitHandle
            isDragging={isDragging}
            onPointerDown={handleDividerPointerDown}
            onDoubleClick={handleDividerDoubleClick}
          />
          <BiPanel />
        </section>
      </div>
    </main>
  );
}

function IconSidebar() {
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
      <nav className="flex flex-1 flex-col items-center gap-2 py-4">
        <SidebarIcon
          icon={<Bot className="size-5" aria-hidden />}
          label="AI"
          active
        />
        <SidebarIcon
          icon={<BarChart3 className="size-5" aria-hidden />}
          label="BI"
        />
      </nav>
    </aside>
  );
}

function SidebarIcon({
  icon,
  label,
  active = false,
}: {
  icon: ReactNode;
  label: string;
  active?: boolean;
}) {
  return (
    <Button
      type="button"
      variant="ghost"
      size="icon"
      className={cn(
        "size-10 rounded-md text-sidebar-foreground/70 hover:bg-sidebar-accent hover:text-sidebar-accent-foreground",
        active &&
          "bg-sidebar-accent text-sidebar-accent-foreground shadow-xs ring-1 ring-sidebar-border",
      )}
      aria-label={label}
      title={label}
    >
      {icon}
    </Button>
  );
}

function AiPanel() {
  return (
    <section className="flex min-h-[calc(100vh-1.5rem)] min-w-0 flex-col rounded-lg border bg-card text-card-foreground shadow-sm lg:h-[calc(100vh-1.5rem)]">
      <header className="flex shrink-0 items-center justify-between gap-3 border-b px-4 py-3">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <h1 className="truncate text-base font-semibold">AI 数据助手</h1>
            <Badge variant="secondary" className="h-6 rounded-md">
              假数据
            </Badge>
          </div>
          <p className="mt-1 truncate text-xs text-muted-foreground">
            使用模拟审计数据回答 BI 与风险治理问题
          </p>
        </div>
        <Button
          type="button"
          variant="outline"
          size="icon"
          aria-label="打开对话记录"
          title="对话记录"
        >
          <MessageSquare className="size-4" aria-hidden />
        </Button>
      </header>

      <div className="flex min-h-0 flex-1 flex-col">
        <div className="flex-1 space-y-4 overflow-y-auto px-4 py-4">
          {chatMessages.map((message) => (
            <ChatBubble key={message.id} message={message} />
          ))}
        </div>

        <div className="border-t bg-card px-4 py-3">
          <div className="mb-3 flex flex-wrap gap-2">
            {quickPrompts.map((prompt) => (
              <Button
                key={prompt}
                type="button"
                variant="outline"
                size="sm"
                className="h-8 rounded-md px-2.5 text-xs"
              >
                {prompt}
              </Button>
            ))}
          </div>
          <div className="flex items-end gap-2 rounded-lg border bg-background p-2 shadow-xs">
            <label className="sr-only" htmlFor="ai-message">
              输入问题
            </label>
            <textarea
              id="ai-message"
              className="max-h-32 min-h-16 flex-1 resize-none bg-transparent px-2 py-1.5 text-sm outline-none placeholder:text-muted-foreground"
              placeholder="输入问题，例如：列出本周风险最高的数据集"
              defaultValue=""
            />
            <Button
              type="button"
              size="icon"
              aria-label="发送问题"
              title="发送"
            >
              <Send className="size-4" aria-hidden />
            </Button>
          </div>
        </div>
      </div>
    </section>
  );
}

function ChatBubble({ message }: { message: ChatMessage }) {
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
        </div>
      </div>
    </article>
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
        aria-label="拖拽调整 AI 与 BI 区域宽度"
        title="拖拽调整宽度，双击恢复默认"
      >
        <span className="h-16 w-1 rounded-full bg-border transition-colors group-hover:bg-foreground/40" />
      </button>
    </div>
  );
}

function BiPanel() {
  return (
    <section className="mt-3 flex min-h-[calc(100vh-1.5rem)] min-w-0 flex-col overflow-hidden rounded-lg border bg-card text-card-foreground shadow-sm lg:mt-0 lg:h-[calc(100vh-1.5rem)]">
      <header className="flex shrink-0 flex-col gap-3 border-b px-4 py-3 xl:flex-row xl:items-center xl:justify-between">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <h2 className="truncate text-base font-semibold">BI 数据看板</h2>
            <Badge variant="outline" className="h-6 rounded-md">
              最近 7 天
            </Badge>
          </div>
          <p className="mt-1 truncate text-xs text-muted-foreground">
            查询趋势、风险分布与数据集明细
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button type="button" variant="outline" size="sm">
            <Search className="size-4" aria-hidden />
            搜索
          </Button>
          <Button type="button" variant="outline" size="sm">
            <FileText className="size-4" aria-hidden />
            导出
          </Button>
          <Button type="button" variant="secondary" size="sm">
            <ChevronDown className="size-4" aria-hidden />
            全部数据集
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
  return (
    <Card className="rounded-lg py-4 shadow-xs">
      <CardHeader className="flex flex-row items-center justify-between px-4">
        <div>
          <CardTitle className="text-sm">查询趋势</CardTitle>
          <p className="mt-1 text-xs text-muted-foreground">
            查询量、风险量和通过率变化
          </p>
        </div>
        <PanelsLeftRight className="size-4 text-muted-foreground" aria-hidden />
      </CardHeader>
      <CardContent className="px-2 pt-2 sm:px-4">
        <div className="h-72">
          <ResponsiveContainer width="100%" height="100%">
            <LineChart data={trendData}>
              <CartesianGrid stroke={chartColors.grid} vertical={false} />
              <XAxis
                dataKey="day"
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
                dataKey="查询量"
                stroke={chartColors.primary}
                strokeWidth={2.5}
                dot={false}
              />
              <Line
                yAxisId="volume"
                type="monotone"
                dataKey="风险量"
                stroke={chartColors.danger}
                strokeWidth={2.5}
                dot={false}
              />
              <Line
                yAxisId="rate"
                type="monotone"
                dataKey="通过率"
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
  return (
    <Card className="rounded-lg py-4 shadow-xs">
      <CardHeader className="flex flex-row items-center justify-between px-4">
        <div>
          <CardTitle className="text-sm">风险分布</CardTitle>
          <p className="mt-1 text-xs text-muted-foreground">
            按数据域聚合的风险事件
          </p>
        </div>
        <TrendingUp className="size-4 text-muted-foreground" aria-hidden />
      </CardHeader>
      <CardContent className="px-2 pt-2 sm:px-4">
        <div className="h-72">
          <ResponsiveContainer width="100%" height="100%">
            <BarChart data={categoryData} layout="vertical">
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
              <Bar dataKey="count" radius={[0, 6, 6, 0]}>
                {categoryData.map((entry) => (
                  <Cell key={entry.name} fill={riskColors[entry.risk]} />
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
  return (
    <Card className="mt-4 rounded-lg py-4 shadow-xs">
      <CardHeader className="flex flex-row items-center justify-between gap-3 px-4">
        <div>
          <CardTitle className="text-sm">数据集明细</CardTitle>
          <p className="mt-1 text-xs text-muted-foreground">
            模拟数据，按风险优先级展示
          </p>
        </div>
        <Table2 className="size-4 shrink-0 text-muted-foreground" aria-hidden />
      </CardHeader>
      <CardContent className="px-0">
        <div className="overflow-x-auto">
          <table className="w-full min-w-[720px] border-collapse text-sm">
            <thead>
              <tr className="border-y bg-muted/60 text-left text-xs text-muted-foreground">
                <th className="px-4 py-2.5 font-medium">数据集</th>
                <th className="px-4 py-2.5 font-medium">负责人</th>
                <th className="px-4 py-2.5 text-right font-medium">查询量</th>
                <th className="px-4 py-2.5 font-medium">风险级别</th>
                <th className="px-4 py-2.5 font-medium">状态</th>
                <th className="px-4 py-2.5 font-medium">更新时间</th>
              </tr>
            </thead>
            <tbody>
              {datasetRows.map((row) => (
                <tr
                  key={row.dataset}
                  className="border-b transition-colors hover:bg-muted/40"
                >
                  <td className="px-4 py-3 font-medium">{row.dataset}</td>
                  <td className="px-4 py-3 text-muted-foreground">
                    {row.owner}
                  </td>
                  <td className="px-4 py-3 text-right tabular-nums">
                    {row.queries.toLocaleString()}
                  </td>
                  <td className="px-4 py-3">
                    <RiskBadge risk={row.risk} />
                  </td>
                  <td className="px-4 py-3">
                    <StatusBadge status={row.status} />
                  </td>
                  <td className="px-4 py-3 text-muted-foreground">
                    {row.updatedAt}
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
  const variant = risk === "严重" || risk === "高" ? "destructive" : "outline";

  return (
    <Badge variant={variant} className="rounded-md">
      {risk}
    </Badge>
  );
}

function StatusBadge({ status }: { status: DatasetStatus }) {
  const statusClasses = {
    正常: "border-chart-2/30 bg-chart-2/10 text-foreground",
    观察: "border-chart-4/30 bg-chart-4/15 text-foreground",
    需处理: "border-destructive/25 bg-destructive/10 text-destructive",
  }[status];

  return (
    <span
      className={cn(
        "inline-flex h-6 items-center rounded-md border px-2 text-xs font-medium",
        statusClasses,
      )}
    >
      {status}
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
