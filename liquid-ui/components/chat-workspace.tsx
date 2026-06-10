"use client";

import {
  Children,
  type ClipboardEvent,
  isValidElement,
  type KeyboardEvent,
  type ReactElement,
  type ReactNode,
  type Ref,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  AlertTriangle,
  Archive,
  Bot,
  Check,
  CheckCircle2,
  Clipboard,
  Copy,
  Database,
  BarChart3,
  FileJson,
  Loader2,
  PanelRightOpen,
  RotateCcw,
  Send,
  Square,
  Table2,
  Trash2,
  X,
} from "lucide-react";
import ReactMarkdown from "react-markdown";
import { Light as SyntaxHighlighter } from "react-syntax-highlighter";
import sqlSyntax from "react-syntax-highlighter/dist/esm/languages/hljs/sql";
import rehypeSanitize from "rehype-sanitize";
import remarkGfm from "remark-gfm";
import { toast } from "sonner";

import { DatapanelChartRenderer } from "@/components/datapanel-chart-renderer";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import {
  type AgentMessageRole,
  type DatapanelCard,
  type ChatAction,
  type ChatConversation,
  type ChatErrorCode,
  type ChatMessage,
  type ChatMessagePart,
  type ChatSqlExecutionResponse,
  type ChatStreamEvent,
  type ChatStreamStage,
  type ChatTurn,
  type DatabaseBackupRecord,
  type DatabaseDiagram,
  type LlmProviderSettingsResponse,
  type ManagedDatabase,
  type PublicUser,
  type SaveDatapanelTableCardRequest,
  type SqlRollbackPlan,
  apiRequest,
  apiStream,
} from "@/lib/api";
import { QueryResultTable } from "@/components/query-result-table";
import { type Locale, useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

type DisplayMessage = ChatMessage & {
  local?: boolean;
};

type FailedTurn = {
  turnId: string;
  prompt: string;
  code: ChatErrorCode;
  message: string;
};

type PendingActionDecision = "apply" | "reject";

type ComposerMode = "chat" | "sql";

type ActivityItem = {
  id: string;
  kind: "status" | "tool";
  stage?: ChatStreamStage;
  name?: string;
  title: string;
  summary: string;
  status: "running" | "succeeded" | "failed";
  elapsedMs?: number;
  outputPreview?: string;
};

type ToolStartedPayload = Extract<
  ChatStreamEvent,
  { type: "tool_started" }
>["payload"];

type ToolFinishedPayload = Extract<
  ChatStreamEvent,
  { type: "tool_finished" }
>["payload"];

type ResourceMentionKind = "database_diagram" | "database_backup";

type ResourceMentionQuery = {
  start: number;
  end: number;
  query: string;
};

type ResourceMentionItem = {
  key: string;
  kind: ResourceMentionKind;
  id: string;
  shortId: string;
  title: string;
  description: string;
  meta: string;
  searchText: string;
};

type ChatHighlightSegment = {
  text: string;
  item?: ResourceMentionItem;
};

type ResourceMentionTokenRange = {
  start: number;
  end: number;
  token: string;
  item: ResourceMentionItem;
};

type ResourceMentionGroup = {
  kind: ResourceMentionKind;
  label: string;
  items: ResourceMentionItem[];
};

type ResourceMentionResources = {
  diagrams: DatabaseDiagram[];
  backups: DatabaseBackupRecord[];
};

type ResourceMentionStatus = "idle" | "loading" | "ready" | "failed";

type ChatPanelProps = {
  token: string;
  user: PublicUser;
  selectedDatabase: ManagedDatabase;
  conversation: ChatConversation;
  isDeletingWorkspace: boolean;
  onConversationUpdated: (conversation: ChatConversation) => void;
  onDatapanelChanged: () => void;
  onDeleteConversation: (conversationId: string) => void | Promise<void>;
};

type CodeElementProps = {
  className?: string;
  children?: ReactNode;
};

const SQL_CODE_LANGUAGES = new Set(["sql", "pgsql", "postgres", "postgresql"]);

SyntaxHighlighter.registerLanguage("sql", sqlSyntax);

export function ChatPanel({
  token,
  user,
  selectedDatabase,
  conversation,
  isDeletingWorkspace,
  onConversationUpdated,
  onDatapanelChanged,
  onDeleteConversation,
}: ChatPanelProps) {
  const { t } = useI18n();
  const [messages, setMessages] = useState<DisplayMessage[]>([]);
  const [actions, setActions] = useState<ChatAction[]>([]);
  const [titleInput, setTitleInput] = useState(conversation.title);
  const [composerMode, setComposerMode] = useState<ComposerMode>("chat");
  const [isLoading, setIsLoading] = useState(true);
  const [isSending, setIsSending] = useState(false);
  const [isSavingTitle, setIsSavingTitle] = useState(false);
  const [isDeleteDialogOpen, setIsDeleteDialogOpen] = useState(false);
  const [providerReady, setProviderReady] = useState<boolean | null>(null);
  const [activityItems, setActivityItems] = useState<ActivityItem[]>([]);
  const [activeTurn, setActiveTurn] = useState<ChatTurn | null>(null);
  const [failedTurn, setFailedTurn] = useState<FailedTurn | null>(null);
  const [pendingActionDecisions, setPendingActionDecisions] = useState<
    Record<string, PendingActionDecision>
  >({});
  const listRef = useRef<HTMLDivElement>(null);
  const activeStreamRef = useRef<AbortController | null>(null);
  const activeActionStreamRef = useRef<AbortController | null>(null);
  const activeConversationStreamRef = useRef<AbortController | null>(null);
  const activeTurnRef = useRef<ChatTurn | null>(null);
  const activeConversationIdRef = useRef(conversation.id);
  const activeSendRef = useRef<string | null>(null);
  const activeActionStreamKeyRef = useRef<string | null>(null);
  const pendingActionIdsRef = useRef(new Set<string>());
  const notifiedDatapanelActionIdsRef = useRef(new Set<string>());
  const loadVersionRef = useRef(0);
  const nearBottomRef = useRef(true);

  const actionGroups = useMemo(() => groupActionsByTurn(actions), [actions]);
  const lastSubmittedPromptByMode = useMemo(
    () => lastSubmittedPromptsByMode(messages),
    [messages],
  );
  const orphanActions = useMemo(() => {
    const assistantTurnIds = new Set(
      messages
        .filter((message) => message.role === "assistant" && message.turn_id)
        .map((message) => message.turn_id),
    );

    return actions.filter((action) => !assistantTurnIds.has(action.turn_id));
  }, [actions, messages]);

  const loadConversationState = useCallback(
    async (conversationId: string) => {
      const [nextMessages, nextActions, provider] = await Promise.all([
        apiRequest<ChatMessage[]>(
          `/api/v1/chat/conversations/${conversationId}/messages?limit=100`,
          { token },
        ),
        apiRequest<ChatAction[]>(
          `/api/v1/chat/conversations/${conversationId}/actions`,
          { token },
        ),
        apiRequest<LlmProviderSettingsResponse>("/api/v1/settings/llm-provider", {
          token,
        }),
      ]);

      return {
        actions: nextActions,
        messages: nextMessages,
        providerReady: Boolean(provider.settings?.has_api_key),
      };
    },
    [token],
  );

  useEffect(() => {
    setTitleInput(conversation.title);
  }, [conversation.id, conversation.title]);

  useEffect(() => {
    let cancelled = false;
    const loadVersion = loadVersionRef.current + 1;

    loadVersionRef.current = loadVersion;
    activeConversationIdRef.current = conversation.id;
    activeStreamRef.current?.abort();
    activeActionStreamRef.current?.abort();
    activeConversationStreamRef.current?.abort();
    activeStreamRef.current = null;
    activeActionStreamRef.current = null;
    activeConversationStreamRef.current = null;
    activeTurnRef.current = null;
    activeSendRef.current = null;
    activeActionStreamKeyRef.current = null;
    pendingActionIdsRef.current.clear();
    notifiedDatapanelActionIdsRef.current.clear();
    nearBottomRef.current = true;
    setComposerMode("chat");
    setMessages([]);
    setActions([]);
    setIsSending(false);
    setActivityItems([]);
    setActiveTurn(null);
    setFailedTurn(null);
    setPendingActionDecisions({});
    setIsLoading(true);

    const load = async () => {
      try {
        const state = await loadConversationState(conversation.id);

        if (cancelled || loadVersionRef.current !== loadVersion) {
          return;
        }

        setMessages(state.messages);
        setActions(state.actions);
        setProviderReady(state.providerReady);
        const lastMessageId = state.messages.at(-1)?.id;
        const streamController = new AbortController();
        activeConversationStreamRef.current = streamController;
        void apiStream<ChatStreamEvent>(
          `/api/v1/chat/conversations/${conversation.id}/stream${
            lastMessageId ? `?after_message_id=${encodeURIComponent(lastMessageId)}` : ""
          }`,
          {
            token,
            signal: streamController.signal,
            onEvent: (event) => {
              if (
                activeConversationIdRef.current !== conversation.id ||
                event.type !== "message_created"
              ) {
                return;
              }

              setMessages((current) => upsertMessage(current, event.payload.message));
            },
          },
        ).catch((error) => {
          if (
            streamController.signal.aborted ||
            activeConversationIdRef.current !== conversation.id
          ) {
            return;
          }
          toast.error(
            error instanceof Error ? error.message : t.workspace.agentLoadFailed,
          );
        });
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

    void load();

    return () => {
      cancelled = true;
      activeStreamRef.current?.abort();
      activeActionStreamRef.current?.abort();
      activeConversationStreamRef.current?.abort();
      activeStreamRef.current = null;
      activeActionStreamRef.current = null;
      activeConversationStreamRef.current = null;
      activeTurnRef.current = null;
      activeSendRef.current = null;
      activeActionStreamKeyRef.current = null;
    };
  }, [conversation.id, loadConversationState, t.workspace.agentLoadFailed, token]);

  useEffect(() => {
    if (!nearBottomRef.current) {
      return;
    }

    const list = listRef.current;

    if (!list) {
      return;
    }

    list.scrollTo({ top: list.scrollHeight, behavior: "smooth" });
  }, [messages, actions, activityItems, isSending]);

  const mergeAction = useCallback((action: ChatAction) => {
    setActions((current) => {
      if (current.some((item) => item.id === action.id)) {
        return current.map((item) => (item.id === action.id ? action : item));
      }

      return [...current, action];
    });
  }, []);

  const notifyDatapanelActionApplied = useCallback(
    (action: ChatAction) => {
      if (
        action.status !== "applied" ||
        action.resource_kind !== "datapanel_card" ||
        notifiedDatapanelActionIdsRef.current.has(action.id)
      ) {
        return;
      }

      notifiedDatapanelActionIdsRef.current.add(action.id);
      onDatapanelChanged();
    },
    [onDatapanelChanged],
  );

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
      const updatedConversation = await apiRequest<ChatConversation>(
        `/api/v1/chat/conversations/${conversation.id}`,
        {
          method: "PATCH",
          token,
          body: { title },
        },
      );
      const mergedConversation = {
        ...conversation,
        title: updatedConversation.title,
        selected_database: updatedConversation.selected_database,
        created_at: updatedConversation.created_at,
        updated_at: updatedConversation.updated_at,
      };

      setTitleInput(updatedConversation.title);
      onConversationUpdated(mergedConversation);
    } catch (error) {
      setTitleInput(conversation.title);
      toast.error(
        error instanceof Error ? error.message : t.workspace.renameFailed,
      );
    } finally {
      setIsSavingTitle(false);
    }
  }, [
    conversation,
    isSavingTitle,
    onConversationUpdated,
    t.workspace.renameFailed,
    titleInput,
    token,
  ]);

  const handleStreamEvent = useCallback(
    (
      event: ChatStreamEvent,
      turn: ChatTurn,
      prompt: string,
      actionStreamActionId?: string,
    ) => {
      switch (event.type) {
        case "turn_started":
          setActivityItems((current) =>
            upsertStatusActivity(current, "planning", undefined, t),
          );
          return false;
        case "message_created":
          setMessages((current) => upsertMessage(current, event.payload.message));
          return false;
        case "status_changed":
          setActivityItems((current) =>
            upsertStatusActivity(
              current,
              event.payload.stage,
              event.payload.summary,
              t,
            ),
          );
          return false;
        case "tool_started":
          setActivityItems((current) =>
            upsertToolStartedActivity(current, event.payload, t),
          );
          return false;
        case "tool_finished":
          setActivityItems((current) =>
            upsertToolFinishedActivity(current, event.payload),
          );
          return false;
        case "assistant_delta": {
          const content =
            event.payload.accumulated === null
              ? event.payload.delta
              : event.payload.accumulated;

          setMessages((current) =>
            upsertStreamingAssistantMessage(
              current,
              turn.id,
              event.payload.message_id,
              content,
            ),
          );
          return false;
        }
        case "assistant_done":
          setActivityItems((current) => completeRunningActivities(current));
          setMessages((current) =>
            upsertAssistantDone(current, event.payload.message, turn.id),
          );
          return false;
        case "action_proposed":
          setActivityItems((current) =>
            upsertStatusActivity(current, "proposing_action", undefined, t),
          );
          mergeAction(event.payload.action);
          return false;
        case "action_updated":
          if (
            actionStreamActionId &&
            event.payload.action.id === actionStreamActionId &&
            event.payload.action.status === "applying"
          ) {
            setActivityItems([
              createStatusActivity("planning", undefined, t),
            ]);
          }
          mergeAction(event.payload.action);
          notifyDatapanelActionApplied(event.payload.action);
          return false;
        case "turn_waiting_for_user":
          setActiveTurn(event.payload.turn);
          setActivityItems((current) =>
            completeRunningActivities(
              upsertStatusActivity(current, "proposing_action", undefined, t),
            ),
          );
          return false;
        case "turn_completed":
          setActiveTurn(event.payload.turn);
          setActivityItems([]);
          return false;
        case "turn_failed": {
          const message = chatErrorMessage(
            event.payload.error_code,
            event.payload.message,
            t,
          );

          setActivityItems((current) => failRunningActivities(current, message));
          setActiveTurn({
            ...turn,
            status: event.payload.error_code === "turn_cancelled" ? "cancelled" : "failed",
            error_code: event.payload.error_code,
            error_message: event.payload.message,
          });
          setFailedTurn({
            turnId: event.payload.turn_id,
            prompt,
            code: event.payload.error_code,
            message,
          });
          setMessages((current) =>
            upsertErrorMessage(
              current,
              event.payload.turn_id,
              event.payload.error_code,
              message,
            ),
          );
          return true;
        }
      }
    },
    [mergeAction, notifyDatapanelActionApplied, t],
  );

  const submitPrompt = useCallback(
    async (prompt: string) => {
      const content = prompt.trim();

      if (!content || isSending || activeSendRef.current) {
        return;
      }

      const conversationId = conversation.id;
      const localUserId = `local-user-${Date.now()}`;
      const sendKey = `${conversationId}:${localUserId}`;
      const localUserMessage: DisplayMessage = {
        id: localUserId,
        role: "user",
        status: "streaming",
        content,
        parts: [{ kind: "text", text: content }],
        created_at: new Date().toISOString(),
        local: true,
      };

      activeSendRef.current = sendKey;
      activeStreamRef.current?.abort();
      setIsSending(true);
      setActivityItems([createStatusActivity("planning", undefined, t)]);
      setActiveTurn(null);
      setFailedTurn(null);
      setMessages((current) => [...current, localUserMessage]);

      try {
        const turn = await apiRequest<ChatTurn>(
          `/api/v1/chat/conversations/${conversationId}/turns`,
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
              client_request_id: localUserId,
            },
          },
        );
        const controller = new AbortController();
        let sawFailure = false;

        activeTurnRef.current = turn;
        activeStreamRef.current = controller;
        setActiveTurn(turn);
        setMessages((current) =>
          current.map((message) =>
            message.id === localUserId
              ? {
                  ...message,
                  id: turn.input_message_id,
                  turn_id: turn.id,
                  status: "complete",
                  local: false,
                }
              : message,
          ),
        );

        await apiStream<ChatStreamEvent>(
          `/api/v1/chat/turns/${turn.id}/stream?after_seq=0`,
          {
            token,
            signal: controller.signal,
            onEvent: (event) => {
              if (
                activeConversationIdRef.current !== conversationId ||
                activeSendRef.current !== sendKey
              ) {
                return;
              }

              sawFailure = handleStreamEvent(event, turn, content) || sawFailure;
            },
          },
        );

        if (
          !sawFailure &&
          activeConversationIdRef.current === conversationId &&
          activeSendRef.current === sendKey
        ) {
          setFailedTurn(null);
        }
      } catch (error) {
        if (error instanceof DOMException && error.name === "AbortError") {
          return;
        }

        if (activeConversationIdRef.current !== conversationId) {
          return;
        }

        const message =
          error instanceof Error ? error.message : t.workspace.sendFailed;
        const turnId = activeTurnRef.current?.id ?? `failed-${localUserId}`;

        setFailedTurn({
          turnId,
          prompt: content,
          code: "provider_request_failed",
          message,
        });
        setMessages((current) =>
          upsertErrorMessage(current, turnId, "provider_request_failed", message),
        );
        toast.error(message);
      } finally {
        if (activeSendRef.current === sendKey) {
          activeSendRef.current = null;
          activeStreamRef.current = null;
          activeTurnRef.current = null;
          setIsSending(false);
        }
      }
    },
    [
      conversation.id,
      handleStreamEvent,
      isSending,
      selectedDatabase.id,
      t,
      token,
    ],
  );

  const submitSql = useCallback(
    async (sqlInput: string) => {
      const sql = sqlInput.trim();

      if (!sql || isSending || activeSendRef.current) {
        return;
      }

      const conversationId = conversation.id;
      const localUserId = `sql-mode-${Date.now()}`;
      const sendKey = `${conversationId}:${localUserId}`;
      const localUserMessage: DisplayMessage = {
        id: localUserId,
        role: "user",
        status: "streaming",
        content: sql,
        parts: [{ kind: "code", language: "sql", code: sql }],
        created_at: new Date().toISOString(),
        local: true,
      };

      activeSendRef.current = sendKey;
      activeStreamRef.current?.abort();
      setIsSending(true);
      setActivityItems([createStatusActivity("executing_sql", undefined, t)]);
      setActiveTurn(null);
      setFailedTurn(null);
      setMessages((current) => [...current, localUserMessage]);

      try {
        const response = await apiRequest<ChatSqlExecutionResponse>(
          `/api/v1/chat/conversations/${conversationId}/sql-executions`,
          {
            method: "POST",
            token,
            body: {
              sql,
              client_request_id: localUserId,
            },
          },
        );

        if (
          activeConversationIdRef.current !== conversationId ||
          activeSendRef.current !== sendKey
        ) {
          return;
        }

        setActiveTurn(response.turn);
        setActivityItems([]);
        setMessages((current) => {
          const userMessage = sqlUserMessage(response.user_message, sql);
          const withUser = current.some((message) => message.id === localUserId)
            ? current.map((message) =>
                message.id === localUserId
                  ? { ...userMessage, local: false }
                  : message,
              )
            : upsertMessage(current, userMessage);

          return upsertMessage(withUser, response.assistant_message);
        });

        if (response.turn.status === "failed") {
          const message =
            response.turn.error_message || t.workspace.sqlExecutionFailed;

          setFailedTurn({
            turnId: response.turn.id,
            prompt: sql,
            code: "storage_error",
            message,
          });
        } else {
          setFailedTurn(null);
        }
      } catch (error) {
        if (activeConversationIdRef.current !== conversationId) {
          return;
        }

        const message =
          error instanceof Error ? error.message : t.workspace.sqlExecutionFailed;
        const turnId = `failed-${localUserId}`;

        setActivityItems([]);
        setFailedTurn({
          turnId,
          prompt: sql,
          code: "storage_error",
          message,
        });
        setMessages((current) =>
          upsertErrorMessage(
            current.map((message) =>
              message.id === localUserId
                ? { ...message, status: "complete", local: false }
                : message,
            ),
            turnId,
            "storage_error",
            message,
          ),
        );
        toast.error(message);
      } finally {
        if (activeSendRef.current === sendKey) {
          activeSendRef.current = null;
          activeStreamRef.current = null;
          activeTurnRef.current = null;
          setIsSending(false);
          setActivityItems([]);
        }
      }
    },
    [conversation.id, isSending, t, token],
  );

  const stopTurn = useCallback(async () => {
    const turn = activeTurnRef.current ?? activeTurn;

    if (!turn) {
      return;
    }

    try {
      await apiRequest<ChatTurn>(`/api/v1/chat/turns/${turn.id}/cancel`, {
        method: "POST",
        token,
      });
    } catch (error) {
      toast.error(error instanceof Error ? error.message : t.workspace.sendFailed);
    } finally {
      activeStreamRef.current?.abort();
      activeStreamRef.current = null;
      activeTurnRef.current = null;
      activeSendRef.current = null;
      setIsSending(false);
      setActivityItems([]);
      setFailedTurn({
        turnId: turn.id,
        prompt: lastUserPrompt(messages),
        code: "turn_cancelled",
        message: t.workspace.errorMessages.turn_cancelled,
      });
      setMessages((current) =>
        upsertErrorMessage(
          current,
          turn.id,
          "turn_cancelled",
          t.workspace.errorMessages.turn_cancelled,
        ),
      );
    }
  }, [activeTurn, messages, t, token]);

  const streamActionTurn = useCallback(
    async (action: ChatAction) => {
      const conversationId = conversation.id;
      const streamKey = `${conversationId}:${action.id}:${Date.now()}`;
      const controller = new AbortController();
      const afterSeq = action.stream_after_seq ?? 0;
      let reachedApplyStart = afterSeq > 0;
      const turn: ChatTurn = {
        id: action.turn_id,
        conversation_id: conversationId,
        status: "running",
        input_message_id: action.turn_id,
      };

      activeActionStreamRef.current?.abort();
      activeActionStreamRef.current = controller;
      activeActionStreamKeyRef.current = streamKey;
      setActivityItems([createStatusActivity("planning", undefined, t)]);
      setFailedTurn(null);

      try {
        await apiStream<ChatStreamEvent>(
          `/api/v1/chat/turns/${action.turn_id}/stream?after_seq=${afterSeq}`,
          {
            token,
            signal: controller.signal,
            onEvent: (event) => {
              if (
                activeConversationIdRef.current !== conversationId ||
                activeActionStreamKeyRef.current !== streamKey
              ) {
                return;
              }

              if (!reachedApplyStart) {
                if (isApplyingActionUpdateEvent(event, action.id)) {
                  reachedApplyStart = true;
                } else {
                  return;
                }
              }

              handleStreamEvent(event, turn, "", action.id);
            },
          },
        );
      } catch (error) {
        if (error instanceof DOMException && error.name === "AbortError") {
          return;
        }

        if (activeConversationIdRef.current !== conversationId) {
          return;
        }

        toast.error(error instanceof Error ? error.message : t.workspace.actionFailed);
        setActivityItems((current) =>
          failRunningActivities(
            current.length > 0
              ? current
              : [createStatusActivity("planning", undefined, t)],
            error instanceof Error ? error.message : t.workspace.actionFailed,
          ),
        );

        try {
          const refreshed = await loadConversationState(conversationId);

          if (activeConversationIdRef.current === conversationId) {
            setMessages(refreshed.messages);
            setActions(refreshed.actions);
            setProviderReady(refreshed.providerReady);
          }
        } catch {
          // The stream error toast already gives the user immediate feedback.
        }
      } finally {
        if (activeActionStreamKeyRef.current === streamKey) {
          activeActionStreamRef.current = null;
          activeActionStreamKeyRef.current = null;
        }
      }
    },
    [
      conversation.id,
      handleStreamEvent,
      loadConversationState,
      t,
      token,
    ],
  );

  const handleActionDecision = async (
    action: ChatAction,
    decision: "apply" | "reject",
  ) => {
    if (pendingActionIdsRef.current.has(action.id)) {
      return;
    }

    pendingActionIdsRef.current.add(action.id);
    setPendingActionDecisions((current) => ({
      ...current,
      [action.id]: decision,
    }));

    try {
      const updated = await apiRequest<ChatAction>(
        `/api/v1/chat/actions/${action.id}/${decision}`,
        {
          method: "POST",
          token,
          body: {},
        },
      );

      mergeAction(updated);
      notifyDatapanelActionApplied(updated);

      if (decision === "apply" && updated.status === "applying") {
        void streamActionTurn(updated);
        return;
      }

      const refreshed = await loadConversationState(conversation.id);

      if (activeConversationIdRef.current === conversation.id) {
        setMessages(refreshed.messages);
        setActions(refreshed.actions);
        setProviderReady(refreshed.providerReady);
      }

      toast.success(
        decision === "apply" ? t.workspace.actionApplied : t.workspace.actionRejected,
      );
    } catch (error) {
      toast.error(error instanceof Error ? error.message : t.workspace.actionFailed);
    } finally {
      pendingActionIdsRef.current.delete(action.id);
      setPendingActionDecisions((current) => {
        const remaining = { ...current };

        delete remaining[action.id];
        return remaining;
      });
    }
  };

  const handleScroll = () => {
    const list = listRef.current;

    if (!list) {
      return;
    }

    nearBottomRef.current =
      list.scrollHeight - list.scrollTop - list.clientHeight < 96;
  };

  return (
    <section className="flex min-h-[calc(100vh-1.5rem)] min-w-0 flex-col overflow-hidden rounded-lg border bg-card text-card-foreground shadow-sm lg:h-[calc(100vh-1.5rem)]">
      <ChatHeader
        conversation={conversation}
        selectedDatabase={selectedDatabase}
        titleInput={titleInput}
        isSavingTitle={isSavingTitle}
        isDeletingWorkspace={isDeletingWorkspace}
        onTitleChange={setTitleInput}
        onTitleCommit={() => void commitWorkspaceTitle()}
        onTitleReset={() => setTitleInput(conversation.title)}
        onDelete={() => setIsDeleteDialogOpen(true)}
      />

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
        <MessageList
          token={token}
          user={user}
          conversationId={conversation.id}
          listRef={listRef}
          messages={messages}
          actionsByTurn={actionGroups}
          orphanActions={orphanActions}
          isLoading={isLoading}
          isSending={isSending}
          activityItems={activityItems}
          selectedDatabase={selectedDatabase}
          providerReady={providerReady}
          pendingActionDecisions={pendingActionDecisions}
          onScroll={handleScroll}
          onPrompt={(prompt) => void submitPrompt(prompt)}
          onActionApply={(action) => void handleActionDecision(action, "apply")}
          onActionReject={(action) => void handleActionDecision(action, "reject")}
          onDatapanelChanged={onDatapanelChanged}
        />

        <MessageComposer
          key={conversation.id}
          token={token}
          selectedDatabase={selectedDatabase}
          mode={composerMode}
          isLoading={isLoading}
          isSending={isSending}
          providerReady={providerReady}
          failedTurn={failedTurn}
          lastSubmittedPrompt={
            composerMode === "sql"
              ? lastSubmittedPromptByMode.sql
              : lastSubmittedPromptByMode.chat
          }
          onModeChange={setComposerMode}
          onSubmit={(prompt) =>
            void (composerMode === "sql" ? submitSql(prompt) : submitPrompt(prompt))
          }
          onStop={() => void stopTurn()}
          onRetry={(prompt) =>
            void (composerMode === "sql" ? submitSql(prompt) : submitPrompt(prompt))
          }
        />
      </div>
    </section>
  );
}

function ChatHeader({
  conversation,
  selectedDatabase,
  titleInput,
  isSavingTitle,
  isDeletingWorkspace,
  onTitleChange,
  onTitleCommit,
  onTitleReset,
  onDelete,
}: {
  conversation: ChatConversation;
  selectedDatabase: ManagedDatabase;
  titleInput: string;
  isSavingTitle: boolean;
  isDeletingWorkspace: boolean;
  onTitleChange: (title: string) => void;
  onTitleCommit: () => void;
  onTitleReset: () => void;
  onDelete: () => void;
}) {
  const { t } = useI18n();

  return (
    <header className="flex shrink-0 items-center justify-between gap-3 border-b bg-card/95 px-4 py-3">
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
            onChange={(event) => onTitleChange(event.target.value)}
            onBlur={onTitleCommit}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                event.currentTarget.blur();
              }

              if (event.key === "Escape") {
                onTitleReset();
                event.currentTarget.blur();
              }
            }}
          />
          {isSavingTitle ? (
            <Loader2 className="size-4 shrink-0 animate-spin text-muted-foreground" />
          ) : null}
        </div>
        <div className="mt-1 flex min-w-0 flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
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
        onClick={onDelete}
      >
        {isDeletingWorkspace ? (
          <Loader2 className="size-4 animate-spin" aria-hidden />
        ) : (
          <Trash2 className="size-4" aria-hidden />
        )}
      </Button>
    </header>
  );
}

const MessageList = ({
  token,
  user,
  conversationId,
  listRef,
  messages,
  actionsByTurn,
  orphanActions,
  isLoading,
  isSending,
  activityItems,
  selectedDatabase,
  providerReady,
  pendingActionDecisions,
  onScroll,
  onPrompt,
  onActionApply,
  onActionReject,
  onDatapanelChanged,
}: {
  token: string;
  user: PublicUser;
  conversationId: string;
  listRef: Ref<HTMLDivElement>;
  messages: DisplayMessage[];
  actionsByTurn: Map<string, ChatAction[]>;
  orphanActions: ChatAction[];
  isLoading: boolean;
  isSending: boolean;
  activityItems: ActivityItem[];
  selectedDatabase: ManagedDatabase;
  providerReady: boolean | null;
  pendingActionDecisions: Record<string, PendingActionDecision>;
  onScroll: () => void;
  onPrompt: (prompt: string) => void;
  onActionApply: (action: ChatAction) => void;
  onActionReject: (action: ChatAction) => void;
  onDatapanelChanged: () => void;
}) => {
  const { t } = useI18n();
  const actionAnchorMessageIds = useMemo(() => {
    const anchoredTurnIds = new Set<string>();
    const messageIds = new Set<string>();

    for (const message of messages) {
      if (
        message.role !== "assistant" ||
        !message.turn_id ||
        anchoredTurnIds.has(message.turn_id) ||
        !(actionsByTurn.get(message.turn_id)?.length)
      ) {
        continue;
      }

      anchoredTurnIds.add(message.turn_id);
      messageIds.add(message.id);
    }

    return messageIds;
  }, [actionsByTurn, messages]);

  return (
    <div
      ref={listRef}
      className="min-h-0 flex-1 overflow-y-auto px-4 py-4"
      onScroll={onScroll}
    >
      {isLoading ? (
        <div className="flex items-center gap-2 text-sm text-muted-foreground">
          <Loader2 className="size-4 animate-spin" aria-hidden />
          {t.workspace.loadingConversation}
        </div>
      ) : null}

      {!isLoading && messages.length === 0 ? (
        <ChatEmptyState
          selectedDatabase={selectedDatabase}
          providerReady={providerReady}
          onPrompt={onPrompt}
        />
      ) : null}

      <div className="space-y-5">
        {messages.map((message) => {
          const messageActions = message.turn_id
            ? actionsByTurn.get(message.turn_id) ?? []
            : [];

          return (
            <MessageStack
              key={message.id}
              token={token}
              user={user}
              conversationId={conversationId}
              message={message}
              actions={
                actionAnchorMessageIds.has(message.id) ? messageActions : []
              }
              selectedDatabase={selectedDatabase}
              pendingActionDecisions={pendingActionDecisions}
              onActionApply={onActionApply}
              onActionReject={onActionReject}
              onDatapanelChanged={onDatapanelChanged}
            />
          );
        })}
      </div>

      {activityItems.length > 0 || isSending ? (
        <ActivityTimeline
          items={
            activityItems.length > 0
              ? activityItems
              : [createStatusActivity("planning", undefined, t)]
          }
        />
      ) : null}

      {orphanActions.length > 0 ? (
        <div className="mt-4 space-y-2">
          {orphanActions.map((action) => (
            <ActionCard
              key={action.id}
              action={action}
              selectedDatabase={selectedDatabase}
              pendingDecision={pendingActionDecisions[action.id]}
              onApply={() => onActionApply(action)}
              onReject={() => onActionReject(action)}
            />
          ))}
        </div>
      ) : null}
    </div>
  );
};

function ChatEmptyState({
  selectedDatabase,
  providerReady,
  onPrompt,
}: {
  selectedDatabase: ManagedDatabase;
  providerReady: boolean | null;
  onPrompt: (prompt: string) => void;
}) {
  const { t } = useI18n();

  return (
    <div className="flex min-h-full items-center justify-center py-8">
      <div className="w-full max-w-md text-center">
        <div className="mx-auto flex size-10 items-center justify-center rounded-md border bg-secondary text-secondary-foreground">
          <Bot className="size-5" aria-hidden />
        </div>
        <h2 className="mt-4 text-base font-semibold">
          {t.workspace.emptyTitle}
        </h2>
        <p className="mx-auto mt-2 max-w-sm text-sm leading-6 text-muted-foreground">
          {t.workspace.emptyDescription(selectedDatabase.name)}
        </p>
        {providerReady === false ? (
          <div className="mx-auto mt-3 flex max-w-sm items-start gap-2 rounded-md border border-destructive/25 bg-destructive/5 p-3 text-left text-xs leading-5 text-destructive">
            <AlertTriangle className="mt-0.5 size-4 shrink-0" aria-hidden />
            <span>{t.workspace.providerSetupHint}</span>
          </div>
        ) : null}
        <div className="mt-4 flex flex-wrap justify-center gap-2">
          {t.workspace.quickPrompts.map((prompt) => (
            <Button
              key={prompt}
              type="button"
              variant="outline"
              size="sm"
              className="h-8 rounded-md px-2.5 text-xs"
              onClick={() => onPrompt(prompt)}
            >
              {prompt}
            </Button>
          ))}
        </div>
      </div>
    </div>
  );
}

function MessageStack({
  token,
  user,
  conversationId,
  message,
  actions,
  selectedDatabase,
  pendingActionDecisions,
  onActionApply,
  onActionReject,
  onDatapanelChanged,
}: {
  token: string;
  user: PublicUser;
  conversationId: string;
  message: DisplayMessage;
  actions: ChatAction[];
  selectedDatabase: ManagedDatabase;
  pendingActionDecisions: Record<string, PendingActionDecision>;
  onActionApply: (action: ChatAction) => void;
  onActionReject: (action: ChatAction) => void;
  onDatapanelChanged: () => void;
}) {
  return (
    <div className="space-y-2">
      <MessageBubble
        token={token}
        user={user}
        conversationId={conversationId}
        message={message}
        onDatapanelChanged={onDatapanelChanged}
      />
      {actions.length > 0 ? (
        <div className="ml-11 space-y-2">
          {actions.map((action) => (
            <ActionCard
              key={action.id}
              action={action}
              selectedDatabase={selectedDatabase}
              pendingDecision={pendingActionDecisions[action.id]}
              onApply={() => onActionApply(action)}
              onReject={() => onActionReject(action)}
            />
          ))}
        </div>
      ) : null}
    </div>
  );
}

function MessageBubble({
  token,
  user,
  conversationId,
  message,
  onDatapanelChanged,
}: {
  token: string;
  user: PublicUser;
  conversationId: string;
  message: DisplayMessage;
  onDatapanelChanged: () => void;
}) {
  const { locale, t } = useI18n();
  const isUser = message.role === "user";
  const isFailed = message.status === "failed";
  const copyText = message.content.trim();
  const senderLabel = isUser ? user.display_name : roleLabel(message.role, t);
  const isUserSqlMessage = isUser && messageHasSqlCodePart(message);

  return (
    <article
      className={cn(
        "group flex min-w-0 gap-3",
        isUser && "justify-end",
      )}
    >
      {!isUser ? (
        <div
          className={cn(
            "mt-1 flex size-8 shrink-0 items-center justify-center rounded-md border bg-secondary text-secondary-foreground",
            isFailed && "border-destructive/25 bg-destructive/10 text-destructive",
          )}
          aria-hidden
        >
          {isFailed ? (
            <AlertTriangle className="size-4" />
          ) : (
            <Bot className="size-4" />
          )}
        </div>
      ) : null}
      <div className={cn("min-w-0", isUser ? "max-w-[82%]" : "max-w-[92%]")}>
        <div
          className={cn(
            "relative min-w-0 text-left text-sm leading-6",
            isUser &&
              !isUserSqlMessage &&
              "rounded-lg bg-primary px-3 py-2 text-primary-foreground shadow-xs",
            !isUser && "pr-9 text-foreground",
            isFailed &&
              "rounded-md border border-destructive/25 bg-destructive/5 px-3 py-2 text-destructive",
          )}
        >
          <MessageContent
            token={token}
            conversationId={conversationId}
            message={message}
            onDatapanelChanged={onDatapanelChanged}
          />
          {!isUser && copyText ? (
            <CopyButton
              text={copyText}
              className="absolute right-0 top-0 opacity-0 transition-opacity group-hover:opacity-100 group-focus-within:opacity-100"
              title={t.workspace.copy}
            />
          ) : null}
        </div>
        <div
          className={cn(
            "mt-1 flex items-center gap-2 text-xs text-muted-foreground",
            isUser && "justify-end",
          )}
        >
          <span>{senderLabel}</span>
          <span>{timeLabel(message.created_at, locale)}</span>
          {message.status === "streaming" ? (
            <span>{t.workspace.pending}</span>
          ) : null}
          {message.local ? <span>{t.workspace.localPending}</span> : null}
        </div>
      </div>
      {isUser ? (
        <div
          className="mt-1 flex size-8 shrink-0 items-center justify-center rounded-md border bg-background text-xs font-semibold text-foreground shadow-xs"
          title={user.email}
          aria-label={user.display_name}
        >
          {userInitials(user)}
        </div>
      ) : null}
    </article>
  );
}

function MessageContent({
  token,
  conversationId,
  message,
  onDatapanelChanged,
}: {
  token: string;
  conversationId: string;
  message: DisplayMessage;
  onDatapanelChanged: () => void;
}) {
  const parts =
    message.parts.length > 0
      ? message.parts
      : [{ kind: "markdown", markdown: message.content } satisfies ChatMessagePart];

  return (
    <div className="min-w-0 space-y-3">
      {parts.map((part, index) => (
        <MessagePart
          key={`${message.id}-${index}`}
          token={token}
          conversationId={conversationId}
          part={part}
          role={message.role}
          onDatapanelChanged={onDatapanelChanged}
        />
      ))}
    </div>
  );
}

function MessagePart({
  token,
  conversationId,
  part,
  role,
  onDatapanelChanged,
}: {
  token: string;
  conversationId: string;
  part: ChatMessagePart;
  role: AgentMessageRole;
  onDatapanelChanged: () => void;
}) {
  const { t } = useI18n();

  switch (part.kind) {
    case "text":
      return <p className="whitespace-pre-wrap break-words">{part.text}</p>;
    case "markdown":
      return <MarkdownContent markdown={part.markdown} />;
    case "code":
      if (role === "user" && normalizeCodeLanguage(part.language) === "sql") {
        return <UserSqlCodeBubble code={part.code} />;
      }

      return <CodeBlock code={part.code} language={part.language} />;
    case "query_result_table":
      return (
        <QueryResultTableCard
          token={token}
          conversationId={conversationId}
          part={part}
          onDatapanelChanged={onDatapanelChanged}
        />
      );
    case "sql_execution_summary":
      return <SqlExecutionSummaryCard part={part} />;
    case "database_backup_status":
      return <DatabaseBackupStatusCard part={part} />;
    case "database_restore_status":
      return <DatabaseRestoreStatusCard part={part} />;
    case "error":
      return (
        <div className="flex items-start gap-2 text-sm">
          <AlertTriangle className="mt-1 size-4 shrink-0" aria-hidden />
          <div>
            <div className="font-medium">{t.workspace.messageFailed}</div>
            <div className="mt-0.5 text-destructive/90">{part.message}</div>
          </div>
        </div>
      );
    case "status":
      return <InlineStageState stage={part.stage} />;
    case "action_ref":
      return null;
  }
}

type DatabaseBackupStatusPart = Extract<
  ChatMessagePart,
  { kind: "database_backup_status" }
>;

function DatabaseBackupStatusCard({
  part,
}: {
  part: DatabaseBackupStatusPart;
}) {
  const { locale } = useI18n();
  const backup = part.backup;
  const succeeded = backup.status === "succeeded";
  const size = backup.storage?.size_bytes;

  return (
    <article className="overflow-hidden rounded-lg border bg-background text-left shadow-xs">
      <header className="flex items-start gap-3 border-b bg-muted/35 px-3 py-2">
        <div
          className={cn(
            "mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-md border bg-background",
            succeeded ? "text-emerald-600" : "text-destructive",
          )}
        >
          {succeeded ? (
            <Database className="size-4" aria-hidden />
          ) : (
            <AlertTriangle className="size-4" aria-hidden />
          )}
        </div>
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <h3 className="text-sm font-medium">Database backup</h3>
            <Badge variant={succeeded ? "outline" : "destructive"}>
              {backup.status}
            </Badge>
          </div>
          <div className="mt-1 flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
            <span>{backup.source.name}</span>
            <span className="font-mono">{backup.id}</span>
            {backup.completed_at ? (
              <span>{dateTimeLabel(backup.completed_at, locale)}</span>
            ) : null}
          </div>
        </div>
      </header>
      <div className="grid gap-2 px-3 py-2 text-xs text-muted-foreground sm:grid-cols-2">
        <span>Storage: {backup.storage?.kind ?? "pending"}</span>
        <span>Size: {size === undefined ? "unknown" : formatBytes(size)}</span>
        <span>Phase: {backup.phase}</span>
        <span>Progress: {backup.progress_percent}%</span>
      </div>
      {backup.error ? (
        <div className="border-t px-3 py-2 text-xs text-destructive">
          {backup.error}
        </div>
      ) : null}
    </article>
  );
}

type DatabaseRestoreStatusPart = Extract<
  ChatMessagePart,
  { kind: "database_restore_status" }
>;

function DatabaseRestoreStatusCard({
  part,
}: {
  part: DatabaseRestoreStatusPart;
}) {
  const { locale } = useI18n();
  const restore = part.restore;
  const succeeded = restore.status === "succeeded";

  return (
    <article className="overflow-hidden rounded-lg border bg-background text-left shadow-xs">
      <header className="flex items-start gap-3 border-b bg-muted/35 px-3 py-2">
        <div
          className={cn(
            "mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-md border bg-background",
            succeeded ? "text-emerald-600" : "text-destructive",
          )}
        >
          {succeeded ? (
            <RotateCcw className="size-4" aria-hidden />
          ) : (
            <AlertTriangle className="size-4" aria-hidden />
          )}
        </div>
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <h3 className="text-sm font-medium">Database restore</h3>
            <Badge variant={succeeded ? "outline" : "destructive"}>
              {restore.status}
            </Badge>
          </div>
          <div className="mt-1 flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
            <span>{restore.target.name}</span>
            <span className="font-mono">{restore.id}</span>
            {restore.completed_at ? (
              <span>{dateTimeLabel(restore.completed_at, locale)}</span>
            ) : null}
          </div>
        </div>
      </header>
      <div className="grid gap-2 px-3 py-2 text-xs text-muted-foreground sm:grid-cols-2">
        <span>Backup: {restore.backup_id}</span>
        <span>Phase: {restore.phase}</span>
        <span>Progress: {restore.progress_percent}%</span>
        <span>Format: {restore.format}</span>
      </div>
      {restore.error ? (
        <div className="border-t px-3 py-2 text-xs text-destructive">
          {restore.error}
        </div>
      ) : null}
    </article>
  );
}

type QueryResultTablePart = Extract<
  ChatMessagePart,
  { kind: "query_result_table" }
>;

function QueryResultTableCard({
  token,
  conversationId,
  part,
  onDatapanelChanged,
}: {
  token: string;
  conversationId: string;
  part: QueryResultTablePart;
  onDatapanelChanged: () => void;
}) {
  const { t } = useI18n();
  const [isSaving, setIsSaving] = useState(false);
  const [savedCardId, setSavedCardId] = useState<string | null>(null);
  const title = part.title?.trim() || t.workspace.queryResult.title;
  const description = part.description?.trim();
  const summary = t.workspace.queryResult.summary(
    part.result.row_count,
    part.result.elapsed_ms,
    part.result.truncated,
  );
  const canSave = part.saveable !== false;

  const saveToDatapanel = async () => {
    if (isSaving || savedCardId) {
      return;
    }

    setIsSaving(true);

    try {
      const body: SaveDatapanelTableCardRequest = {
        managed_database_id: part.managed_database_id,
        title,
        description,
        sql: part.sql,
        result: part.result,
      };
      const card = await apiRequest<DatapanelCard>(
        `/api/v1/chat/conversations/${conversationId}/datapanel/cards`,
        {
          method: "POST",
          token,
          body,
        },
      );

      setSavedCardId(card.id);
      onDatapanelChanged();
      toast.success(t.workspace.queryResult.savedToast);
    } catch (error) {
      toast.error(
        error instanceof Error
          ? error.message
          : t.workspace.queryResult.saveFailed,
      );
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <article className="overflow-hidden rounded-lg border bg-background text-left shadow-xs">
      <header className="flex flex-col gap-2 border-b bg-muted/35 px-3 py-2 sm:flex-row sm:items-start sm:justify-between">
        <div className="min-w-0">
          <div className="flex min-w-0 items-center gap-2">
            <Table2 className="size-4 shrink-0 text-muted-foreground" aria-hidden />
            <h3 className="truncate text-sm font-medium">{title}</h3>
          </div>
          <div className="mt-1 flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
            <span>{summary}</span>
            {description ? <span>{description}</span> : null}
          </div>
        </div>
        {canSave ? (
          <Button
            type="button"
            variant={savedCardId ? "outline" : "secondary"}
            size="sm"
            className="h-8 shrink-0 rounded-md"
            disabled={isSaving || Boolean(savedCardId)}
            onClick={() => void saveToDatapanel()}
          >
            {isSaving ? (
              <Loader2 className="size-4 animate-spin" aria-hidden />
            ) : savedCardId ? (
              <Check className="size-4" aria-hidden />
            ) : (
              <PanelRightOpen className="size-4" aria-hidden />
            )}
            {isSaving
              ? t.workspace.queryResult.saving
              : savedCardId
                ? t.workspace.queryResult.saved
                : t.workspace.queryResult.save}
          </Button>
        ) : null}
      </header>
      <div className="h-64 min-h-0 p-2">
        <QueryResultTable
          result={part.result}
          emptyLabel={t.workspace.queryResult.empty}
        />
      </div>
      <div className="border-t bg-muted/20 px-3 py-2">
        <CodeBlock code={part.sql} language="sql" />
      </div>
      <RollbackPlanPanel rollback={part.rollback} />
    </article>
  );
}

type SqlExecutionSummaryPart = Extract<
  ChatMessagePart,
  { kind: "sql_execution_summary" }
>;

function SqlExecutionSummaryCard({
  part,
}: {
  part: SqlExecutionSummaryPart;
}) {
  const { t } = useI18n();
  const affectedRows =
    part.affected_rows === undefined
      ? t.workspace.sqlExecutionAffectedRowsUnknown
      : t.workspace.sqlExecutionAffectedRows(part.affected_rows);

  return (
    <article className="overflow-hidden rounded-lg border bg-background text-left shadow-xs">
      <header className="flex items-start gap-3 border-b bg-muted/35 px-3 py-2">
        <div className="mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-md border bg-background text-emerald-600">
          <CheckCircle2 className="size-4" aria-hidden />
        </div>
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <h3 className="text-sm font-medium">
              {t.workspace.sqlExecutionSummaryTitle}
            </h3>
            <Badge variant="outline" className="font-mono uppercase">
              {part.statement_kind}
            </Badge>
          </div>
          <div className="mt-1 flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
            <span>{affectedRows}</span>
            <span>{t.workspace.sqlExecutionElapsed(part.elapsed_ms)}</span>
          </div>
        </div>
      </header>
      <div className="border-t-0 bg-muted/20 px-3 py-2">
        <CodeBlock code={part.sql} language="sql" />
      </div>
      <RollbackPlanPanel rollback={part.rollback} />
    </article>
  );
}

function RollbackPlanPanel({
  rollback,
}: {
  rollback?: SqlRollbackPlan | null;
}) {
  const { t } = useI18n();

  if (!rollback) {
    return null;
  }

  return (
    <div className="border-t bg-background px-3 py-3">
      <div className="mb-2 flex flex-wrap items-center gap-2">
        <RotateCcw className="size-4 text-muted-foreground" aria-hidden />
        <span className="text-xs font-medium uppercase text-muted-foreground">
          {t.workspace.rollbackTitle}
        </span>
        <Badge variant="outline" className="font-mono">
          {t.workspace.rollbackStatuses[rollback.status]}
        </Badge>
        {rollback.generated_at ? (
          <span className="text-xs text-muted-foreground">
            {rollback.generated_at}
          </span>
        ) : null}
      </div>
      {rollback.reason ? (
        <p className="mb-2 break-words text-xs leading-5 text-muted-foreground">
          {rollback.reason}
        </p>
      ) : null}
      {rollback.sql ? <CodeBlock code={rollback.sql} language="sql" /> : null}
    </div>
  );
}

function MarkdownContent({ markdown }: { markdown: string }) {
  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm]}
      rehypePlugins={[rehypeSanitize]}
      components={{
        a: ({ children, href }) => (
          <a
            href={href}
            className="font-medium text-primary underline-offset-4 hover:underline"
            target="_blank"
            rel="noreferrer"
          >
            {children}
          </a>
        ),
        p: ({ children }) => (
          <p className="whitespace-pre-wrap break-words">{children}</p>
        ),
        ul: ({ children }) => (
          <ul className="list-disc space-y-1 pl-5">{children}</ul>
        ),
        ol: ({ children }) => (
          <ol className="list-decimal space-y-1 pl-5">{children}</ol>
        ),
        li: ({ children }) => <li className="pl-1">{children}</li>,
        blockquote: ({ children }) => (
          <blockquote className="border-l-2 pl-3 text-muted-foreground">
            {children}
          </blockquote>
        ),
        table: ({ children }) => (
          <div className="overflow-x-auto rounded-md border">
            <table className="w-full min-w-max border-collapse text-left text-xs">
              {children}
            </table>
          </div>
        ),
        th: ({ children }) => (
          <th className="border-b bg-muted px-2 py-1.5 font-medium">
            {children}
          </th>
        ),
        td: ({ children }) => (
          <td className="border-b px-2 py-1.5 align-top">{children}</td>
        ),
        pre: ({ children }) => {
          const extracted = extractPreCode(children);

          if (!extracted) {
            return (
              <pre className="overflow-x-auto rounded-md border bg-muted p-3 text-xs">
                {children}
              </pre>
            );
          }

          return (
            <CodeBlock code={extracted.code} language={extracted.language} />
          );
        },
        code: ({ children, className }) => (
          <code
            className={cn(
              "rounded-sm bg-muted px-1 py-0.5 font-mono text-[0.92em]",
              className,
            )}
          >
            {children}
          </code>
        ),
      }}
    >
      {markdown}
    </ReactMarkdown>
  );
}

function CodeBlock({
  code,
  language,
}: {
  code: string;
  language: string | null;
}) {
  const { t } = useI18n();
  const label = language?.trim() || t.workspace.codeBlock;
  const normalizedLanguage = normalizeCodeLanguage(language);

  return (
    <div className="overflow-hidden rounded-md border bg-muted/60 text-left">
      <div className="flex h-8 items-center justify-between border-b bg-muted px-3">
        <span className="text-[11px] font-medium uppercase text-muted-foreground">
          {label}
        </span>
        <CopyButton text={code} title={t.workspace.copyCode} compact />
      </div>
      {normalizedLanguage === "sql" ? (
        <SyntaxHighlighter
          language="sql"
          useInlineStyles={false}
          className="liquid-code-highlight max-h-72 overflow-auto p-3 text-xs leading-5"
          codeTagProps={{ className: "font-mono" }}
          customStyle={{ margin: 0, background: "transparent" }}
        >
          {code}
        </SyntaxHighlighter>
      ) : (
        <pre className="max-h-72 overflow-auto p-3 text-xs leading-5">
          <code className="font-mono">{code}</code>
        </pre>
      )}
    </div>
  );
}

function UserSqlCodeBubble({ code }: { code: string }) {
  return (
    <div className="overflow-hidden rounded-lg border border-white/10 bg-neutral-950 text-left text-neutral-50 shadow-sm ring-1 ring-black/20">
      <SyntaxHighlighter
        language="sql"
        useInlineStyles={false}
        className="liquid-user-sql-highlight max-h-72 overflow-auto px-3 py-2 text-sm leading-6"
        codeTagProps={{
          className: "font-mono whitespace-pre-wrap break-words",
        }}
        customStyle={{
          margin: 0,
          background: "transparent",
          whiteSpace: "pre-wrap",
        }}
      >
        {code}
      </SyntaxHighlighter>
    </div>
  );
}

function normalizeCodeLanguage(language: string | null) {
  const normalized = language?.trim().toLowerCase();

  if (!normalized) {
    return null;
  }

  return SQL_CODE_LANGUAGES.has(normalized) ? "sql" : normalized;
}

function messageHasSqlCodePart(message: DisplayMessage) {
  return message.parts.some(
    (part) => part.kind === "code" && normalizeCodeLanguage(part.language) === "sql",
  );
}

function lastSubmittedPromptsByMode(messages: DisplayMessage[]) {
  let chat = "";
  let sql = "";

  for (let index = messages.length - 1; index >= 0 && (!chat || !sql); index -= 1) {
    const message = messages[index];

    if (message.role !== "user") {
      continue;
    }

    const sqlPrompt = sqlPromptFromMessage(message);

    if (sqlPrompt) {
      sql ||= sqlPrompt;
      continue;
    }

    if (!chat && message.content.trim()) {
      chat = message.content;
    }
  }

  return { chat, sql };
}

function sqlPromptFromMessage(message: DisplayMessage) {
  const part = message.parts.find(
    (item) =>
      item.kind === "code" && normalizeCodeLanguage(item.language) === "sql",
  );

  return part?.kind === "code" && part.code.trim() ? part.code : "";
}

function sqlUserMessage(message: ChatMessage, sql: string): DisplayMessage {
  return {
    ...message,
    parts: [{ kind: "code", language: "sql", code: sql }],
  };
}

function ActionCard({
  action,
  selectedDatabase,
  pendingDecision,
  onApply,
  onReject,
}: {
  action: ChatAction;
  selectedDatabase: ManagedDatabase;
  pendingDecision?: PendingActionDecision;
  onApply: () => void;
  onReject: () => void;
}) {
  const { t } = useI18n();
  const isProposed = action.status === "proposed";
  const isApplying = action.status === "applying";
  const isBusy = Boolean(pendingDecision) || isApplying;
  const isActionable = !isBusy && (isProposed || action.status === "failed");
  const databaseName =
    action.preview?.kind === "sql_audit"
      ? action.preview.database_name ?? selectedDatabase.name
      : selectedDatabase.name;
  const applyLabel =
    action.status === "failed"
      ? t.workspace.retry
      : action.preview?.kind === "datapanel_card"
        ? t.workspace.importToDatapanel
        : t.workspace.confirm;
  const applyingLabel =
    action.preview?.kind === "datapanel_card"
      ? t.workspace.importingToDatapanel
      : t.workspace.confirming;

  return (
    <article
      className={cn(
        "rounded-lg border bg-background p-3 shadow-xs transition-colors",
        isBusy && "border-primary/25 bg-primary/5",
      )}
      aria-busy={isBusy}
    >
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
        <Badge
          variant={isProposed || isApplying ? "secondary" : "outline"}
          className={cn("rounded-md", isBusy && "gap-1.5")}
        >
          {isBusy ? (
            <Loader2 className="size-3 animate-spin" aria-hidden />
          ) : null}
          {isBusy
            ? t.workspace.actionProcessing
            : t.workspace.actionStatuses[action.status]}
        </Badge>
      </div>

      {action.preview?.kind === "sql_audit" ? (
        <div className="mt-3 space-y-2">
          <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
            <Clipboard className="size-3.5" aria-hidden />
            <span>{t.workspace.sqlPreview}</span>
            <span className="text-foreground">{databaseName}</span>
          </div>
          <CodeBlock code={action.preview.sql} language="sql" />
          {action.preview.context ? (
            <p className="text-xs leading-5 text-muted-foreground">
              {action.preview.context}
            </p>
          ) : null}
        </div>
      ) : null}

      {action.preview?.kind === "datapanel_card" ? (
        <DatapanelActionPreview action={action} />
      ) : null}

      <div className="mt-3 flex flex-wrap gap-2">
        {isActionable ? (
          <>
            <Button
              type="button"
              size="sm"
              disabled={isBusy}
              onClick={onApply}
            >
              {pendingDecision === "apply" ? (
                <Loader2 className="size-4 animate-spin" aria-hidden />
              ) : action.preview?.kind === "datapanel_card" ? (
                <PanelRightOpen className="size-4" aria-hidden />
              ) : (
                <CheckCircle2 className="size-4" aria-hidden />
              )}
              {pendingDecision === "apply" ? applyingLabel : applyLabel}
            </Button>
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={isBusy}
              onClick={onReject}
            >
              {pendingDecision === "reject" ? (
                <Loader2 className="size-4 animate-spin" aria-hidden />
              ) : (
                <X className="size-4" aria-hidden />
              )}
              {pendingDecision === "reject"
                ? t.workspace.rejecting
                : t.workspace.reject}
            </Button>
          </>
        ) : (
          <div className="flex items-center gap-2 text-xs text-muted-foreground">
            {isApplying ? (
              <>
                <Loader2 className="size-3.5 animate-spin" aria-hidden />
                {t.workspace.actionProcessing}
              </>
            ) : (
              <>
                <Check className="size-3.5" aria-hidden />
                {t.workspace.actionStatusUpdated}
              </>
            )}
          </div>
        )}
      </div>
    </article>
  );
}

function DatapanelActionPreview({ action }: { action: ChatAction }) {
  const { t } = useI18n();

  if (action.preview?.kind !== "datapanel_card") {
    return null;
  }

  const preview = action.preview;

  return (
    <div className="mt-3 space-y-2">
      <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
        {preview.card_kind === "chart" ? (
          <BarChart3 className="size-3.5" aria-hidden />
        ) : (
          <Table2 className="size-3.5" aria-hidden />
        )}
        <span>{t.workspace.datapanelPreview}</span>
        <span className="text-foreground">{preview.title}</span>
        <Badge variant="outline" className="h-5 rounded-md px-1.5 text-[11px]">
          {t.workspace.datapanelRows(preview.result.row_count)}
        </Badge>
      </div>
      {preview.description ? (
        <p className="text-xs leading-5 text-muted-foreground">
          {preview.description}
        </p>
      ) : null}
      <div className="overflow-hidden rounded-md border bg-background">
        <div className="h-44 p-2">
          {preview.card_kind === "chart" && preview.chart ? (
            <MiniDatapanelChart preview={preview} />
          ) : (
            <MiniDatapanelTable preview={preview} />
          )}
        </div>
      </div>
      <CodeBlock code={preview.sql} language="sql" />
    </div>
  );
}

function MiniDatapanelTable({
  preview,
}: {
  preview: Extract<NonNullable<ChatAction["preview"]>, { kind: "datapanel_card" }>;
}) {
  const { t } = useI18n();

  return (
    <QueryResultTable
      result={preview.result}
      maxRows={5}
      stickyHeader={false}
      emptyLabel={t.workspace.queryResult.empty}
      className="rounded-sm"
    />
  );
}

function MiniDatapanelChart({
  preview,
}: {
  preview: Extract<NonNullable<ChatAction["preview"]>, { kind: "datapanel_card" }>;
}) {
  const { t } = useI18n();
  const chart = preview.chart;

  if (!chart) {
    return <MiniDatapanelTable preview={preview} />;
  }

  return (
    <DatapanelChartRenderer
      chart={chart}
      rows={preview.result.rows}
      variant="preview"
      emptyLabel={t.workspace.queryResult.empty}
    />
  );
}

function ActivityTimeline({ items }: { items: ActivityItem[] }) {
  const { t } = useI18n();

  return (
    <div className="mt-4 pl-11">
      <div className="relative">
        {items.length > 1 ? (
          <div
            className="absolute bottom-3 left-[7px] top-3 w-px bg-border"
            aria-hidden
          />
        ) : null}
        <div className="space-y-0">
          {items.map((item) => (
            <div key={item.id} className="relative flex gap-3 pb-3 last:pb-0">
              <ActivityMarker status={item.status} />
              <div className="min-w-0 flex-1 pt-0.5">
                <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
                  <span className="truncate text-sm font-medium text-foreground">
                    {item.kind === "tool"
                      ? toolTitle(item.name, item.title, t)
                      : item.title}
                  </span>
                  {item.kind === "tool" && item.elapsedMs !== undefined ? (
                    <span className="text-[11px] text-muted-foreground">
                      {formatElapsed(item.elapsedMs)}
                    </span>
                  ) : null}
                  {item.status === "failed" ? (
                    <span className="text-[11px] text-destructive">
                      {t.workspace.toolStatuses.failed}
                    </span>
                  ) : null}
                </div>
                <p className="mt-0.5 break-words text-xs leading-5 text-muted-foreground">
                  {item.summary}
                </p>
                {item.outputPreview ? (
                  <p className="mt-1 truncate rounded-sm bg-muted/60 px-2 py-1 font-mono text-[11px] text-muted-foreground">
                    {item.outputPreview}
                  </p>
                ) : null}
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

function ActivityMarker({
  status,
}: {
  status: ActivityItem["status"];
}) {
  if (status === "running") {
    return (
      <span className="relative mt-1 flex size-4 shrink-0">
        <span className="absolute inline-flex size-full animate-ping rounded-full bg-primary/25" />
        <span className="relative inline-flex size-4 rounded-full border border-primary/30 bg-background" />
      </span>
    );
  }

  if (status === "failed") {
    return (
      <span className="relative mt-1 flex size-4 shrink-0 items-center justify-center rounded-full border border-destructive/30 bg-background text-destructive">
        <AlertTriangle className="size-3" aria-hidden />
      </span>
    );
  }

  return (
    <span className="relative mt-1 flex size-4 shrink-0 items-center justify-center rounded-full border border-emerald-500/30 bg-background text-emerald-600">
      <Check className="size-3" aria-hidden />
    </span>
  );
}

function InlineStageState({ stage }: { stage: ChatStreamStage }) {
  const { t } = useI18n();

  return (
    <div className="flex items-center gap-2 py-1 text-sm text-muted-foreground">
      <span className="relative flex size-4">
        <span className="absolute inline-flex size-full animate-ping rounded-full bg-primary/25" />
        <span className="relative inline-flex size-4 rounded-full border border-primary/30 bg-primary/15" />
      </span>
      <span>{t.workspace.stages[stage]}</span>
    </div>
  );
}

const MessageComposer = ({
  token,
  selectedDatabase,
  mode,
  isLoading,
  isSending,
  providerReady,
  failedTurn,
  lastSubmittedPrompt,
  onModeChange,
  onSubmit,
  onStop,
  onRetry,
}: {
  token: string;
  selectedDatabase: ManagedDatabase;
  mode: ComposerMode;
  isLoading: boolean;
  isSending: boolean;
  providerReady: boolean | null;
  failedTurn: FailedTurn | null;
  lastSubmittedPrompt?: string;
  onModeChange: (mode: ComposerMode) => void;
  onSubmit: (prompt: string) => void;
  onStop: () => void;
  onRetry: (prompt: string) => void;
}) => {
  const { locale, t } = useI18n();
  const [input, setInput] = useState("");
  const [mentionResources, setMentionResources] =
    useState<ResourceMentionResources>({
      diagrams: [],
      backups: [],
    });
  const [mentionStatus, setMentionStatus] =
    useState<ResourceMentionStatus>("idle");
  const [mentionError, setMentionError] = useState<string | null>(null);
  const [mentionQuery, setMentionQuery] =
    useState<ResourceMentionQuery | null>(null);
  const [activeMentionKey, setActiveMentionKey] = useState<string | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const mentionLoadRequestedRef = useRef(false);
  const mentionLoadKeyRef = useRef("");
  const mentionLoadAbortRef = useRef<AbortController | null>(null);
  const composerMountedRef = useRef(true);
  const canSubmit = Boolean(input.trim()) && !isLoading && !isSending;
  const isSqlMode = mode === "sql";

  const mentionItems = useMemo(
    () => buildResourceMentionItems(mentionResources, locale, t),
    [locale, mentionResources, t],
  );
  const mentionGroups = useMemo(
    () =>
      groupResourceMentionItems(mentionItems, mentionQuery?.query ?? "", t),
    [mentionItems, mentionQuery?.query, t],
  );
  const flattenedMentionItems = useMemo(
    () => mentionGroups.flatMap((group) => group.items),
    [mentionGroups],
  );

  useEffect(() => {
    composerMountedRef.current = true;

    return () => {
      composerMountedRef.current = false;
      mentionLoadAbortRef.current?.abort();
      mentionLoadAbortRef.current = null;
    };
  }, []);

  useEffect(() => {
    if (mentionStatus !== "loading") {
      return;
    }

    const timeoutId = window.setTimeout(() => {
      if (mentionStatus !== "loading") {
        return;
      }

      mentionLoadAbortRef.current?.abort();
      mentionLoadAbortRef.current = null;
      mentionLoadRequestedRef.current = false;
      setMentionError(t.workspace.resourceMentions.loadTimedOut);
      setMentionStatus("failed");
    }, RESOURCE_MENTION_LOAD_TIMEOUT_MS + 1000);

    return () => window.clearTimeout(timeoutId);
  }, [mentionStatus, t.workspace.resourceMentions.loadTimedOut]);

  useEffect(() => {
    mentionLoadAbortRef.current?.abort();
    mentionLoadAbortRef.current = null;
    mentionLoadRequestedRef.current = false;
    mentionLoadKeyRef.current = "";
    setMentionResources({ diagrams: [], backups: [] });
    setMentionStatus("idle");
    setMentionError(null);
    setMentionQuery(null);
    setActiveMentionKey(null);
  }, [selectedDatabase.id, token]);

  useEffect(() => {
    const textarea = textareaRef.current;

    if (!textarea) {
      return;
    }

    textarea.style.height = "auto";
    textarea.style.height = `${Math.min(textarea.scrollHeight, 168)}px`;
  }, [input, mode]);

  useEffect(() => {
    if (!isSqlMode) {
      return;
    }

    setMentionQuery(null);
    setActiveMentionKey(null);
  }, [isSqlMode]);

  useEffect(() => {
    if (!mentionQuery || flattenedMentionItems.length === 0) {
      setActiveMentionKey(null);
      return;
    }

    if (!flattenedMentionItems.some((item) => item.key === activeMentionKey)) {
      setActiveMentionKey(flattenedMentionItems[0]?.key ?? null);
    }
  }, [activeMentionKey, flattenedMentionItems, mentionQuery]);

  const loadMentionResources = useCallback((force = false) => {
    if (mentionLoadRequestedRef.current && !force) {
      return;
    }

    mentionLoadAbortRef.current?.abort();

    const loadKey = `${token}:${selectedDatabase.id}:${Date.now()}`;
    const controller = new AbortController();
    let didTimeout = false;
    const timeoutId = window.setTimeout(() => {
      didTimeout = true;
      controller.abort();
    }, RESOURCE_MENTION_LOAD_TIMEOUT_MS);
    const backupParams = new URLSearchParams({
      managed_database_id: selectedDatabase.id,
      page: "1",
      page_size: "100",
    });

    mentionLoadRequestedRef.current = true;
    mentionLoadKeyRef.current = loadKey;
    mentionLoadAbortRef.current = controller;
    setMentionStatus("loading");
    setMentionError(null);

    void Promise.all([
      apiRequest<DatabaseDiagram[]>("/api/v1/database-diagrams", {
        token,
        signal: controller.signal,
      }),
      apiRequest<DatabaseBackupRecord[]>(
        `/api/v1/database-backups?${backupParams.toString()}`,
        { token, signal: controller.signal },
      ),
    ])
      .then(([diagrams, backups]) => {
        if (
          !composerMountedRef.current ||
          mentionLoadKeyRef.current !== loadKey
        ) {
          return;
        }

        mentionLoadRequestedRef.current = true;
        setMentionResources({ diagrams, backups });
        setMentionStatus("ready");
      })
      .catch((error) => {
        if (
          !composerMountedRef.current ||
          mentionLoadKeyRef.current !== loadKey
        ) {
          return;
        }

        mentionLoadRequestedRef.current = false;
        setMentionError(
          didTimeout
            ? t.workspace.resourceMentions.loadTimedOut
            : error instanceof DOMException && error.name === "AbortError"
              ? t.workspace.resourceMentions.loadFailed
              : error instanceof Error
                ? error.message
                : t.workspace.resourceMentions.loadFailed,
        );
        setMentionStatus("failed");
      })
      .finally(() => {
        window.clearTimeout(timeoutId);

        if (mentionLoadAbortRef.current === controller) {
          mentionLoadAbortRef.current = null;
        }
      });
  }, [
    selectedDatabase.id,
    t.workspace.resourceMentions.loadFailed,
    t.workspace.resourceMentions.loadTimedOut,
    token,
  ]);

  const updateMentionQuery = useCallback(
    (value: string, caretPosition: number | null) => {
      if (isSqlMode || caretPosition === null) {
        setMentionQuery(null);
        return;
      }

      const nextQuery = findActiveResourceMention(value, caretPosition);

      setMentionQuery(nextQuery);

      if (nextQuery) {
        loadMentionResources();
      }
    },
    [isSqlMode, loadMentionResources],
  );

  const refreshMentionQueryFromTextarea = useCallback(() => {
    const textarea = textareaRef.current;

    if (!textarea) {
      return;
    }

    updateMentionQuery(textarea.value, textarea.selectionStart);
  }, [updateMentionQuery]);

  const selectMentionItem = useCallback(
    (item: ResourceMentionItem) => {
      if (!mentionQuery) {
        return;
      }

      const token = resourceMentionVisibleToken(item.kind, item.shortId);
      const before = input.slice(0, mentionQuery.start);
      const after = input.slice(mentionQuery.end);
      const separator = after && !/^[\s,.;:!?，。；：！？)]/.test(after) ? " " : "";
      const nextInput = `${before}${token}${separator}${after}`;
      const caretPosition = before.length + token.length + separator.length;

      setInput(nextInput);
      setMentionQuery(null);
      setActiveMentionKey(null);

      requestAnimationFrame(() => {
        const textarea = textareaRef.current;

        if (!textarea) {
          return;
        }

        textarea.focus();
        textarea.setSelectionRange(caretPosition, caretPosition);
      });
    },
    [input, mentionQuery],
  );

  const moveActiveMention = useCallback(
    (direction: 1 | -1) => {
      if (flattenedMentionItems.length === 0) {
        return;
      }

      const currentIndex = Math.max(
        0,
        flattenedMentionItems.findIndex((item) => item.key === activeMentionKey),
      );
      const nextIndex =
        (currentIndex + direction + flattenedMentionItems.length) %
        flattenedMentionItems.length;

      setActiveMentionKey(flattenedMentionItems[nextIndex]?.key ?? null);
    },
    [activeMentionKey, flattenedMentionItems],
  );

  const replaceMentionTokenSelection = useCallback(
    (
      replacement: string,
      deleteDirection?: "backward" | "forward",
    ) => {
      const textarea = textareaRef.current;

      if (!textarea || isSqlMode) {
        return false;
      }

      const value = textarea.value;
      const selectionStart = textarea.selectionStart;
      const selectionEnd = textarea.selectionEnd;
      const ranges = resourceMentionTokenRanges(value, mentionItems);

      if (ranges.length === 0) {
        return false;
      }

      let replaceStart = selectionStart;
      let replaceEnd = selectionEnd;

      if (selectionStart === selectionEnd) {
        const range =
          deleteDirection === "backward"
            ? ranges.find(
                (candidate) =>
                  selectionStart > candidate.start && selectionStart <= candidate.end,
              )
            : deleteDirection === "forward"
              ? ranges.find(
                  (candidate) =>
                    selectionStart >= candidate.start &&
                    selectionStart < candidate.end,
                )
              : ranges.find(
                  (candidate) =>
                    selectionStart > candidate.start && selectionStart < candidate.end,
                );

        if (!range) {
          return false;
        }

        replaceStart = range.start;
        replaceEnd = range.end;
      } else {
        const touchedRanges = ranges.filter(
          (range) => selectionStart < range.end && selectionEnd > range.start,
        );

        if (touchedRanges.length === 0) {
          return false;
        }

        replaceStart = Math.min(
          selectionStart,
          ...touchedRanges.map((range) => range.start),
        );
        replaceEnd = Math.max(
          selectionEnd,
          ...touchedRanges.map((range) => range.end),
        );
      }

      const nextInput =
        value.slice(0, replaceStart) + replacement + value.slice(replaceEnd);
      const nextCaret = replaceStart + replacement.length;

      setInput(nextInput);
      setMentionQuery(null);
      setActiveMentionKey(null);

      requestAnimationFrame(() => {
        const nextTextarea = textareaRef.current;

        if (!nextTextarea) {
          return;
        }

        nextTextarea.focus();
        nextTextarea.setSelectionRange(nextCaret, nextCaret);
      });

      return true;
    },
    [isSqlMode, mentionItems],
  );

  const submitInput = () => {
    const prompt = input.trim();

    if (!prompt || isLoading || isSending) {
      return;
    }

    setInput("");
    setMentionQuery(null);
    setActiveMentionKey(null);
    onSubmit(isSqlMode ? prompt : expandResourceMentionTokens(prompt, mentionItems));
  };

  const handleCut = useCallback(
    (event: ClipboardEvent<HTMLTextAreaElement>) => {
      const textarea = textareaRef.current;

      if (!textarea || textarea.selectionStart === textarea.selectionEnd) {
        return;
      }

      const selectedText = textarea.value.slice(
        textarea.selectionStart,
        textarea.selectionEnd,
      );

      if (replaceMentionTokenSelection("")) {
        event.preventDefault();
        event.clipboardData.setData("text/plain", selectedText);
      }
    },
    [replaceMentionTokenSelection],
  );

  const handlePaste = useCallback(
    (event: ClipboardEvent<HTMLTextAreaElement>) => {
      const replacement = event.clipboardData.getData("text/plain");

      if (replaceMentionTokenSelection(replacement)) {
        event.preventDefault();
      }
    },
    [replaceMentionTokenSelection],
  );

  const handleKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.nativeEvent.isComposing) {
      return;
    }

    if (mentionQuery && !isSqlMode) {
      if (event.key === "Escape") {
        event.preventDefault();
        setMentionQuery(null);
        setActiveMentionKey(null);
        return;
      }

      if (event.key === "ArrowDown" && flattenedMentionItems.length > 0) {
        event.preventDefault();
        moveActiveMention(1);
        return;
      }

      if (event.key === "ArrowUp" && flattenedMentionItems.length > 0) {
        event.preventDefault();
        moveActiveMention(-1);
        return;
      }

      if (
        (event.key === "Enter" || event.key === "Tab") &&
        flattenedMentionItems.length > 0
      ) {
        const item =
          flattenedMentionItems.find((candidate) => candidate.key === activeMentionKey) ??
          flattenedMentionItems[0];

        if (item) {
          event.preventDefault();
          selectMentionItem(item);
          return;
        }
      }
    }

    if (
      event.key === "Backspace" &&
      replaceMentionTokenSelection("", "backward")
    ) {
      event.preventDefault();
      return;
    }

    if (event.key === "Delete" && replaceMentionTokenSelection("", "forward")) {
      event.preventDefault();
      return;
    }

    if (
      event.key.length === 1 &&
      !event.altKey &&
      !event.ctrlKey &&
      !event.metaKey &&
      replaceMentionTokenSelection(event.key)
    ) {
      event.preventDefault();
      return;
    }

    if (event.key === "ArrowUp") {
      const prompt = lastSubmittedPrompt?.trim() ? lastSubmittedPrompt : "";

      if (input.trim() || !prompt) {
        return;
      }

      event.preventDefault();
      setInput(prompt);
      setMentionQuery(null);
      setActiveMentionKey(null);

      requestAnimationFrame(() => {
        const textarea = textareaRef.current;

        if (!textarea) {
          return;
        }

        textarea.focus();
        textarea.setSelectionRange(prompt.length, prompt.length);
      });
      return;
    }

    if (event.key !== "Enter" || event.shiftKey) {
      return;
    }

    event.preventDefault();
    submitInput();
  };

  return (
    <footer className="shrink-0 border-t bg-card px-4 py-3">
      {failedTurn ? (
        <div className="mb-3 flex items-start justify-between gap-3 rounded-md border border-destructive/25 bg-destructive/5 p-3 text-sm">
          <div className="flex min-w-0 items-start gap-2 text-destructive">
            <AlertTriangle className="mt-0.5 size-4 shrink-0" aria-hidden />
            <div className="min-w-0">
              <div className="font-medium">{t.workspace.messageFailed}</div>
              <div className="mt-0.5 break-words text-xs leading-5">
                {failedTurn.message}
              </div>
            </div>
          </div>
          {failedTurn.prompt ? (
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="h-8 shrink-0 rounded-md"
              onClick={() =>
                onRetry(
                  isSqlMode
                    ? failedTurn.prompt
                    : expandResourceMentionTokens(failedTurn.prompt, mentionItems),
                )
              }
            >
              <RotateCcw className="size-4" aria-hidden />
              {t.workspace.retry}
            </Button>
          ) : null}
        </div>
      ) : null}

      {providerReady === false && !isSqlMode ? (
        <div className="mb-3 flex items-center gap-2 text-xs text-muted-foreground">
          <AlertTriangle className="size-3.5 text-destructive" aria-hidden />
          <span>{t.workspace.providerSetupHint}</span>
        </div>
      ) : null}

      <form
        className="relative rounded-lg border bg-background p-2 shadow-xs focus-within:ring-[3px] focus-within:ring-ring/50"
        onSubmit={(event) => {
          event.preventDefault();
          submitInput();
        }}
      >
        {mentionQuery && !isSqlMode ? (
          <ResourceMentionPanel
            groups={mentionGroups}
            status={mentionStatus}
            query={mentionQuery.query}
            error={mentionError}
            activeKey={activeMentionKey}
            onActiveKeyChange={setActiveMentionKey}
            onRetry={() => loadMentionResources(true)}
            onSelect={selectMentionItem}
          />
        ) : null}
        <label className="sr-only" htmlFor="ai-message">
          {isSqlMode ? t.workspace.sqlInputLabel : t.workspace.inputLabel}
        </label>
        {isSqlMode ? (
          <SqlHighlightedTextarea
            textareaRef={textareaRef}
            input={input}
            isLoading={isLoading}
            placeholder={t.workspace.sqlInputPlaceholder}
            onChange={setInput}
            onKeyDown={handleKeyDown}
          />
        ) : (
          <ChatHighlightedTextarea
            textareaRef={textareaRef}
            input={input}
            isLoading={isLoading}
            placeholder={t.workspace.inputPlaceholder}
            mentionItems={mentionItems}
            onChange={(value, caretPosition) => {
              setInput(value);
              updateMentionQuery(value, caretPosition);
            }}
            onClick={refreshMentionQueryFromTextarea}
            onCut={handleCut}
            onKeyDown={handleKeyDown}
            onKeyUp={refreshMentionQueryFromTextarea}
            onPaste={handlePaste}
          />
        )}
        <div className="mt-2 flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
          <div className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1">
            <label className="flex shrink-0 items-center gap-2 px-2 text-xs font-medium text-muted-foreground">
              <Switch
                checked={isSqlMode}
                disabled={isLoading || isSending}
                aria-label={t.workspace.sqlModeSwitchLabel}
                onCheckedChange={(checked) =>
                  onModeChange(checked ? "sql" : "chat")
                }
              />
              <span>{isSqlMode ? t.workspace.sqlModeLabel : t.workspace.chatModeLabel}</span>
            </label>
            <span className="min-w-0 truncate px-2 text-xs text-muted-foreground">
              {isSending
                ? isSqlMode
                  ? t.workspace.sqlExecuting
                  : t.workspace.pending
                : isSqlMode
                  ? t.workspace.sqlComposerHint
                  : t.workspace.composerHint}
            </span>
          </div>
          {isSending && !isSqlMode ? (
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="h-8 rounded-md"
              onClick={onStop}
            >
              <Square className="size-3.5 fill-current" aria-hidden />
              {t.workspace.stop}
            </Button>
          ) : isSending ? (
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="h-8 rounded-md"
              disabled
            >
              <Loader2 className="size-4 animate-spin" aria-hidden />
              {t.workspace.sqlExecuting}
            </Button>
          ) : (
            <TooltipProvider>
              <Tooltip>
                <TooltipTrigger asChild>
                  <span className="inline-flex">
                    <Button
                      type="submit"
                      size="sm"
                      className="h-8 rounded-md"
                      aria-label={
                        isSqlMode
                          ? t.workspace.sqlExecute
                          : t.workspace.sendQuestion
                      }
                      disabled={!canSubmit}
                    >
                      <Send className="size-4" aria-hidden />
                      {t.workspace.send}
                    </Button>
                  </span>
                </TooltipTrigger>
                <TooltipContent side="top">
                  {isSqlMode
                    ? t.workspace.sqlExecuteShortcut
                    : t.workspace.chatSendShortcut}
                </TooltipContent>
              </Tooltip>
            </TooltipProvider>
          )}
        </div>
      </form>
    </footer>
  );
};

function ResourceMentionPanel({
  groups,
  status,
  query,
  error,
  activeKey,
  onActiveKeyChange,
  onRetry,
  onSelect,
}: {
  groups: ResourceMentionGroup[];
  status: ResourceMentionStatus;
  query: string;
  error: string | null;
  activeKey: string | null;
  onActiveKeyChange: (key: string) => void;
  onRetry: () => void;
  onSelect: (item: ResourceMentionItem) => void;
}) {
  const { t } = useI18n();
  const hasResults = groups.some((group) => group.items.length > 0);

  return (
    <div
      className="absolute bottom-[calc(100%+0.5rem)] left-0 right-0 z-30 max-h-80 overflow-hidden rounded-lg border bg-popover text-popover-foreground shadow-lg"
      role="listbox"
      aria-label={t.workspace.resourceMentions.panelLabel}
    >
      <div className="flex items-center justify-between gap-3 border-b px-3 py-2 text-xs text-muted-foreground">
        <span className="truncate">
          {query
            ? t.workspace.resourceMentions.searching(query)
            : t.workspace.resourceMentions.searchHint}
        </span>
        <span className="shrink-0 font-mono">@</span>
      </div>

      <div className="max-h-72 overflow-y-auto py-1">
        {status === "loading" ? (
          <ResourceMentionState>
            <Loader2 className="size-4 animate-spin" aria-hidden />
            {t.workspace.resourceMentions.loading}
          </ResourceMentionState>
        ) : status === "failed" ? (
          <div className="flex items-center justify-between gap-3 px-3 py-3 text-sm text-muted-foreground">
            <div className="flex min-w-0 items-center gap-2">
              <AlertTriangle className="size-4 shrink-0 text-destructive" aria-hidden />
              <span className="truncate">
                {error ?? t.workspace.resourceMentions.loadFailed}
              </span>
            </div>
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="h-7 shrink-0 rounded-md"
              onMouseDown={(event) => {
                event.preventDefault();
                onRetry();
              }}
            >
              {t.workspace.resourceMentions.retry}
            </Button>
          </div>
        ) : hasResults ? (
          groups.map((group) =>
            group.items.length > 0 ? (
              <section key={group.kind} className="py-1">
                <div className="px-3 py-1 text-[11px] font-medium text-muted-foreground">
                  {group.label}
                </div>
                <div className="space-y-0.5 px-1">
                  {group.items.map((item) => (
                    <button
                      key={item.key}
                      type="button"
                      role="option"
                      aria-selected={item.key === activeKey}
                      className={cn(
                        "flex w-full min-w-0 items-start gap-2 rounded-md px-2 py-2 text-left outline-none transition-colors",
                        item.key === activeKey
                          ? "bg-accent text-accent-foreground"
                          : "hover:bg-muted/60",
                      )}
                      onMouseEnter={() => onActiveKeyChange(item.key)}
                      onMouseDown={(event) => {
                        event.preventDefault();
                        onSelect(item);
                      }}
                    >
                      <span className="mt-0.5 flex size-7 shrink-0 items-center justify-center rounded-md border bg-background text-muted-foreground">
                        {item.kind === "database_backup" ? (
                          <Archive className="size-3.5" aria-hidden />
                        ) : (
                          <FileJson className="size-3.5" aria-hidden />
                        )}
                      </span>
                      <span className="min-w-0 flex-1">
                        <span className="block truncate text-sm font-medium">
                          {item.title}
                        </span>
                        <span className="mt-0.5 block truncate text-xs text-muted-foreground">
                          {item.description}
                        </span>
                        <span className="mt-0.5 block truncate font-mono text-[11px] text-muted-foreground">
                          {item.meta}
                        </span>
                      </span>
                    </button>
                  ))}
                </div>
              </section>
            ) : null,
          )
        ) : (
          <ResourceMentionState>
            {status === "ready"
              ? t.workspace.resourceMentions.noResults
              : t.workspace.resourceMentions.empty}
          </ResourceMentionState>
        )}
      </div>
    </div>
  );
}

function ResourceMentionState({ children }: { children: ReactNode }) {
  return (
    <div className="flex items-center gap-2 px-3 py-4 text-sm text-muted-foreground">
      {children}
    </div>
  );
}

function ChatHighlightedTextarea({
  textareaRef,
  input,
  isLoading,
  placeholder,
  mentionItems,
  onChange,
  onClick,
  onCut,
  onKeyDown,
  onKeyUp,
  onPaste,
}: {
  textareaRef: Ref<HTMLTextAreaElement>;
  input: string;
  isLoading: boolean;
  placeholder: string;
  mentionItems: ResourceMentionItem[];
  onChange: (value: string, caretPosition: number | null) => void;
  onClick: () => void;
  onCut: (event: ClipboardEvent<HTMLTextAreaElement>) => void;
  onKeyDown: (event: KeyboardEvent<HTMLTextAreaElement>) => void;
  onKeyUp: () => void;
  onPaste: (event: ClipboardEvent<HTMLTextAreaElement>) => void;
}) {
  const highlightedInput = input || " ";
  const segments = chatHighlightSegments(highlightedInput, mentionItems);

  return (
    <div className="relative max-h-[10.5rem] min-h-12 overflow-hidden">
      <div
        aria-hidden
        className="pointer-events-none absolute inset-0 overflow-hidden px-2 py-1.5 text-sm leading-6 whitespace-pre-wrap break-words text-foreground"
      >
        {segments.map((segment, index) =>
          segment.item ? (
            <span
              key={`${segment.item.key}-${index}`}
              className={cn(
                "rounded-md px-1 py-0.5 font-medium ring-1 [-webkit-box-decoration-break:clone] [box-decoration-break:clone]",
                segment.item.kind === "database_backup"
                  ? "bg-primary/10 text-primary ring-primary/25"
                  : "bg-secondary text-secondary-foreground ring-border",
              )}
            >
              {segment.text}
            </span>
          ) : (
            <span key={`text-${index}`}>{segment.text}</span>
          ),
        )}
      </div>
      <textarea
        ref={textareaRef}
        id="ai-message"
        className="relative z-10 max-h-[10.5rem] min-h-12 w-full resize-none bg-transparent px-2 py-1.5 text-sm leading-6 text-transparent caret-foreground outline-none selection:bg-primary/20 placeholder:text-muted-foreground"
        placeholder={placeholder}
        value={input}
        disabled={isLoading}
        rows={1}
        onChange={(event) =>
          onChange(event.target.value, event.currentTarget.selectionStart)
        }
        onClick={onClick}
        onCut={onCut}
        onKeyDown={onKeyDown}
        onKeyUp={onKeyUp}
        onPaste={onPaste}
      />
    </div>
  );
}

const RESOURCE_MENTION_LIMIT = 5;
const RESOURCE_MENTION_LOAD_TIMEOUT_MS = 8000;
const RESOURCE_MENTION_SEND_LABELS: Record<ResourceMentionKind, string> = {
  database_backup: "备份记录",
  database_diagram: "数据库设计记录",
};

function buildResourceMentionItems(
  resources: ResourceMentionResources,
  locale: Locale,
  t: ReturnType<typeof useI18n>["t"],
): ResourceMentionItem[] {
  const ids = [
    ...resources.diagrams.map((diagram) => diagram.id),
    ...resources.backups.map((backup) => backup.id),
  ];

  const diagrams = resources.diagrams.map((diagram) => {
    const shortId = uniqueResourceMentionId(diagram.id, ids);
    const stats = t.workspace.resourceMentions.designDescription(
      diagram.document.tables.length,
      diagram.document.relationships.length,
    );

    return {
      key: `database_diagram:${diagram.id}`,
      kind: "database_diagram" as const,
      id: diagram.id,
      shortId,
      title: diagram.title,
      description: diagram.description ?? stats,
      meta: `${t.workspace.resourceMentions.updatedAt(
        dateTimeLabel(diagram.updated_at, locale),
      )} · ${resourceMentionVisibleToken("database_diagram", shortId)}`,
      searchText: mentionSearchText([
        diagram.id,
        diagram.title,
        diagram.description,
      ]),
    };
  });

  const backups = resources.backups.map((backup) => {
    const shortId = uniqueResourceMentionId(backup.id, ids);
    const statusLabel = t.backupHistory.statuses[backup.status];

    return {
      key: `database_backup:${backup.id}`,
      kind: "database_backup" as const,
      id: backup.id,
      shortId,
      title: backup.purpose ?? t.workspace.resourceMentions.backupTitle(shortId),
      description: `${backup.source.name} / ${backup.source.database}`,
      meta: `${statusLabel} · ${t.workspace.resourceMentions.createdAt(
        dateTimeLabel(backup.created_at, locale),
      )} · ${resourceMentionVisibleToken("database_backup", shortId)}`,
      searchText: mentionSearchText([
        backup.id,
        backup.purpose,
        backup.source.name,
        backup.source.database,
        backup.source.host,
        backup.source.username,
        backup.status,
        statusLabel,
        backup.phase,
        backup.trigger,
        backup.storage?.kind,
        backup.storage?.local_path,
        backup.storage?.bucket,
        backup.storage?.key,
      ]),
    };
  });

  return [...diagrams, ...backups];
}

function groupResourceMentionItems(
  items: ResourceMentionItem[],
  query: string,
  t: ReturnType<typeof useI18n>["t"],
): ResourceMentionGroup[] {
  const normalizedQuery = normalizeMentionSearch(query);
  const matches = (item: ResourceMentionItem) =>
    !normalizedQuery || item.searchText.includes(normalizedQuery);

  return [
    {
      kind: "database_diagram",
      label: t.workspace.resourceMentions.groups.databaseDesign,
      items: items
        .filter((item) => item.kind === "database_diagram" && matches(item))
        .slice(0, RESOURCE_MENTION_LIMIT),
    },
    {
      kind: "database_backup",
      label: t.workspace.resourceMentions.groups.backups,
      items: items
        .filter((item) => item.kind === "database_backup" && matches(item))
        .slice(0, RESOURCE_MENTION_LIMIT),
    },
  ];
}

function chatHighlightSegments(
  value: string,
  mentionItems: ResourceMentionItem[],
): ChatHighlightSegment[] {
  const ranges = resourceMentionTokenRanges(value, mentionItems);

  if (ranges.length === 0) {
    return [{ text: value }];
  }

  const segments: ChatHighlightSegment[] = [];
  let cursor = 0;

  for (const range of ranges) {
    if (range.start > cursor) {
      segments.push({ text: value.slice(cursor, range.start) });
    }

    segments.push({ text: range.token, item: range.item });
    cursor = range.end;
  }

  if (cursor < value.length) {
    segments.push({ text: value.slice(cursor) });
  }

  return segments.length > 0 ? segments : [{ text: value }];
}

function resourceMentionTokenRanges(
  value: string,
  mentionItems: ResourceMentionItem[],
): ResourceMentionTokenRange[] {
  const tokens = mentionItems
    .map((item) => ({
      item,
      token: resourceMentionVisibleToken(item.kind, item.shortId),
    }))
    .sort((left, right) => right.token.length - left.token.length);

  if (tokens.length === 0) {
    return [];
  }

  const ranges: ResourceMentionTokenRange[] = [];
  let cursor = 0;

  while (cursor < value.length) {
    let nextMatch:
      | { index: number; token: string; item: ResourceMentionItem }
      | null = null;

    for (const candidate of tokens) {
      const index = value.indexOf(candidate.token, cursor);

      if (index < 0) {
        continue;
      }

      if (
        !nextMatch ||
        index < nextMatch.index ||
        (index === nextMatch.index && candidate.token.length > nextMatch.token.length)
      ) {
        nextMatch = {
          index,
          token: candidate.token,
          item: candidate.item,
        };
      }
    }

    if (!nextMatch) {
      break;
    }

    ranges.push({
      start: nextMatch.index,
      end: nextMatch.index + nextMatch.token.length,
      token: nextMatch.token,
      item: nextMatch.item,
    });
    cursor = nextMatch.index + nextMatch.token.length;
  }

  return ranges;
}

function findActiveResourceMention(
  value: string,
  caretPosition: number,
): ResourceMentionQuery | null {
  const beforeCaret = value.slice(0, caretPosition);
  const atIndex = beforeCaret.lastIndexOf("@");

  if (atIndex < 0) {
    return null;
  }

  const query = beforeCaret.slice(atIndex + 1);

  if (!query || /[\s/]/.test(query)) {
    return query === "" ? { start: atIndex, end: caretPosition, query } : null;
  }

  return {
    start: atIndex,
    end: caretPosition,
    query,
  };
}

function expandResourceMentionTokens(
  value: string,
  items: ResourceMentionItem[],
): string {
  return [...items]
    .sort(
      (left, right) =>
        resourceMentionVisibleToken(right.kind, right.shortId).length -
        resourceMentionVisibleToken(left.kind, left.shortId).length,
    )
    .reduce(
      (current, item) =>
        current
          .split(resourceMentionVisibleToken(item.kind, item.shortId))
          .join(resourceMentionExpandedToken(item.kind, item.id)),
      value,
    );
}

function resourceMentionVisibleToken(
  kind: ResourceMentionKind,
  shortId: string,
) {
  return `@${RESOURCE_MENTION_SEND_LABELS[kind]}/${shortId}`;
}

function resourceMentionExpandedToken(kind: ResourceMentionKind, id: string) {
  return `${RESOURCE_MENTION_SEND_LABELS[kind]}/${id}`;
}

function uniqueResourceMentionId(id: string, ids: string[]) {
  const minimumLength = Math.min(8, id.length);

  for (let length = minimumLength; length <= id.length; length += 1) {
    const prefix = id.slice(0, length);

    if (ids.every((candidate) => candidate === id || !candidate.startsWith(prefix))) {
      return prefix;
    }
  }

  return id;
}

function mentionSearchText(values: Array<string | null | undefined>) {
  return normalizeMentionSearch(values.filter(Boolean).join(" "));
}

function normalizeMentionSearch(value: string) {
  return value.toLocaleLowerCase();
}

function SqlHighlightedTextarea({
  textareaRef,
  input,
  isLoading,
  placeholder,
  onChange,
  onKeyDown,
}: {
  textareaRef: Ref<HTMLTextAreaElement>;
  input: string;
  isLoading: boolean;
  placeholder: string;
  onChange: (value: string) => void;
  onKeyDown: (event: KeyboardEvent<HTMLTextAreaElement>) => void;
}) {
  const highlightedInput = input || " ";

  return (
    <div className="relative max-h-[10.5rem] min-h-12 overflow-hidden">
      <SyntaxHighlighter
        language="sql"
        useInlineStyles={false}
        className="liquid-code-highlight pointer-events-none absolute inset-0 overflow-hidden px-2 py-1.5 text-sm leading-6"
        codeTagProps={{
          className: "font-mono whitespace-pre-wrap break-words",
        }}
        customStyle={{
          margin: 0,
          background: "transparent",
          whiteSpace: "pre-wrap",
        }}
      >
        {highlightedInput}
      </SyntaxHighlighter>
      <textarea
        ref={textareaRef}
        id="ai-message"
        className="relative z-10 max-h-[10.5rem] min-h-12 w-full resize-none bg-transparent px-2 py-1.5 font-mono text-sm leading-6 text-transparent caret-foreground outline-none selection:bg-primary/20 placeholder:font-sans placeholder:text-muted-foreground"
        placeholder={placeholder}
        value={input}
        disabled={isLoading}
        rows={1}
        spellCheck={false}
        onChange={(event) => onChange(event.target.value)}
        onKeyDown={onKeyDown}
      />
    </div>
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

function CopyButton({
  text,
  title,
  compact = false,
  className,
}: {
  text: string;
  title: string;
  compact?: boolean;
  className?: string;
}) {
  const { t } = useI18n();

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      toast.success(t.workspace.copied);
    } catch (error) {
      toast.error(error instanceof Error ? error.message : t.workspace.copyFailed);
    }
  };

  return (
    <Button
      type="button"
      variant="ghost"
      size="icon"
      className={cn(
        compact ? "size-7" : "size-8",
        "rounded-md text-muted-foreground hover:text-foreground",
        className,
      )}
      aria-label={title}
      title={title}
      onClick={() => void copy()}
    >
      <Copy className="size-3.5" aria-hidden />
    </Button>
  );
}

function createStatusActivity(
  stage: ChatStreamStage,
  summary: string | undefined,
  t: ReturnType<typeof useI18n>["t"],
): ActivityItem {
  return {
    id: `status-${stage}`,
    kind: "status",
    stage,
    title: t.workspace.stages[stage],
    summary: summary?.trim() || t.workspace.stageSummaries[stage],
    status: "running",
  };
}

function upsertStatusActivity(
  items: ActivityItem[],
  stage: ChatStreamStage,
  summary: string | undefined,
  t: ReturnType<typeof useI18n>["t"],
): ActivityItem[] {
  const next = items.map((item) =>
    item.kind === "status" && item.status === "running"
      ? { ...item, status: "succeeded" as const }
      : item,
  );
  const activity = createStatusActivity(stage, summary, t);
  const existingIndex = next.findIndex((item) => item.id === activity.id);

  if (existingIndex === -1) {
    return [...next, activity];
  }

  return next.map((item, index) =>
    index === existingIndex ? { ...item, ...activity } : item,
  );
}

function upsertToolStartedActivity(
  items: ActivityItem[],
  payload: ToolStartedPayload,
  t: ReturnType<typeof useI18n>["t"],
): ActivityItem[] {
  const next = items.map((item) =>
    item.kind === "status" && item.status === "running"
      ? { ...item, status: "succeeded" as const }
      : item,
  );
  const activity: ActivityItem = {
    id: `tool-${payload.id}`,
    kind: "tool",
    name: payload.name,
    title: toolTitle(payload.name, payload.title, t),
    summary: payload.summary || t.workspace.toolStatuses.running,
    status: "running",
  };
  const existingIndex = next.findIndex((item) => item.id === activity.id);

  if (existingIndex === -1) {
    return [...next, activity];
  }

  return next.map((item, index) =>
    index === existingIndex ? { ...item, ...activity } : item,
  );
}

function upsertToolFinishedActivity(
  items: ActivityItem[],
  payload: ToolFinishedPayload,
): ActivityItem[] {
  const status = payload.status === "failed" ? "failed" : "succeeded";
  const nextActivity: ActivityItem = {
    id: `tool-${payload.id}`,
    kind: "tool",
    name: payload.name,
    title: payload.name.replaceAll("_", " "),
    summary: payload.summary,
    status,
    elapsedMs: payload.elapsed_ms,
    outputPreview: payload.output_preview,
  };
  const existingIndex = items.findIndex((item) => item.id === nextActivity.id);

  if (existingIndex === -1) {
    return [...items, nextActivity];
  }

  return items.map((item, index) =>
    index === existingIndex
      ? {
          ...item,
          summary: payload.summary,
          status,
          elapsedMs: payload.elapsed_ms,
          outputPreview: payload.output_preview,
        }
      : item,
  );
}

function completeRunningActivities(items: ActivityItem[]): ActivityItem[] {
  return items.map((item) =>
    item.status === "running" ? { ...item, status: "succeeded" } : item,
  );
}

function failRunningActivities(
  items: ActivityItem[],
  message: string,
): ActivityItem[] {
  if (items.length === 0) {
    return [];
  }

  return items.map((item, index) =>
    item.status === "running" || index === items.length - 1
      ? {
          ...item,
          status: "failed",
          summary: message || item.summary,
        }
      : item,
  );
}

function isApplyingActionUpdateEvent(
  event: ChatStreamEvent,
  actionId: string,
): boolean {
  return (
    event.type === "action_updated" &&
    event.payload.action.id === actionId &&
    event.payload.action.status === "applying"
  );
}

function toolTitle(
  name: string | undefined,
  fallback: string,
  t: ReturnType<typeof useI18n>["t"],
) {
  if (name && name in t.workspace.toolTitles) {
    return t.workspace.toolTitles[name as keyof typeof t.workspace.toolTitles];
  }

  return fallback || t.workspace.toolTitles.tool;
}

function formatElapsed(elapsedMs: number) {
  if (elapsedMs < 1000) {
    return `${elapsedMs}ms`;
  }

  return `${(elapsedMs / 1000).toFixed(1)}s`;
}

function groupActionsByTurn(actions: ChatAction[]) {
  const groups = new Map<string, ChatAction[]>();

  for (const action of actions) {
    const group = groups.get(action.turn_id) ?? [];
    group.push(action);
    groups.set(action.turn_id, group);
  }

  return groups;
}

function upsertMessage(
  messages: DisplayMessage[],
  nextMessage: ChatMessage,
): DisplayMessage[] {
  if (messages.some((message) => message.id === nextMessage.id)) {
    return messages.map((message) =>
      message.id === nextMessage.id ? { ...nextMessage, local: false } : message,
    );
  }

  return [...messages, nextMessage];
}

function upsertStreamingAssistantMessage(
  messages: DisplayMessage[],
  turnId: string,
  messageId: string,
  content: string,
): DisplayMessage[] {
  const id = messageId || `stream-${turnId}`;
  const nextMessage: DisplayMessage = {
    id,
    role: "assistant",
    status: "streaming",
    content,
    parts: [{ kind: "markdown", markdown: content }],
    turn_id: turnId,
    created_at: new Date().toISOString(),
    local: true,
  };

  if (messages.some((message) => message.id === id)) {
    return messages.map((message) =>
      message.id === id
        ? {
            ...message,
            content,
            parts: [{ kind: "markdown", markdown: content }],
            status: "streaming",
          }
        : message,
    );
  }

  return [...messages, nextMessage];
}

function upsertAssistantDone(
  messages: DisplayMessage[],
  finalMessage: ChatMessage,
  turnId: string,
): DisplayMessage[] {
  const withoutStreaming = messages.filter(
    (message) =>
      !(
        message.role === "assistant" &&
        message.turn_id === turnId &&
        message.status === "streaming"
      ),
  );

  if (withoutStreaming.some((message) => message.id === finalMessage.id)) {
    return withoutStreaming.map((message) =>
      message.id === finalMessage.id
        ? { ...finalMessage, status: "complete", local: false }
        : message,
    );
  }

  return [...withoutStreaming, { ...finalMessage, status: "complete" }];
}

function upsertErrorMessage(
  messages: DisplayMessage[],
  turnId: string,
  code: ChatErrorCode,
  message: string,
): DisplayMessage[] {
  const id = `error-${turnId}`;
  const errorMessage: DisplayMessage = {
    id,
    role: "assistant",
    status: "failed",
    content: message,
    parts: [{ kind: "error", code, message }],
    turn_id: turnId,
    created_at: new Date().toISOString(),
    local: true,
  };

  if (messages.some((item) => item.id === id)) {
    return messages.map((item) => (item.id === id ? errorMessage : item));
  }

  return [...messages, errorMessage];
}

function chatErrorMessage(
  code: ChatErrorCode,
  backendMessage: string,
  t: ReturnType<typeof useI18n>["t"],
) {
  if (code in t.workspace.errorMessages) {
    return t.workspace.errorMessages[code];
  }

  return backendMessage || t.workspace.errorMessages.storage_error;
}

function roleLabel(
  role: AgentMessageRole,
  t: ReturnType<typeof useI18n>["t"],
) {
  if (role === "user") {
    return t.workspace.userLabel;
  }

  if (role === "assistant") {
    return t.workspace.assistantLabel;
  }

  if (role === "tool") {
    return t.workspace.toolLabel;
  }

  return role;
}

function userInitials(user: PublicUser) {
  return (
    user.display_name
      .split(/\s+/)
      .filter(Boolean)
      .slice(0, 2)
      .map((part) => part[0])
      .join("")
      .toUpperCase() || user.email.slice(0, 1).toUpperCase()
  );
}

function lastUserPrompt(messages: DisplayMessage[]) {
  return (
    [...messages].reverse().find((message) => message.role === "user")?.content ??
    ""
  );
}

function timeLabel(value: string, locale: Locale): string {
  return new Date(value).toLocaleTimeString(locale, {
    hour: "2-digit",
    minute: "2-digit",
  });
}

function dateTimeLabel(value: string, locale: Locale): string {
  return new Date(value).toLocaleString(locale, {
    month: "short",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function formatBytes(value: number): string {
  if (!Number.isFinite(value) || value < 0) {
    return "unknown";
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

function extractPreCode(children: ReactNode) {
  const child = Children.toArray(children)[0];

  if (!isValidElement(child)) {
    return null;
  }

  const element = child as ReactElement<CodeElementProps>;
  const className = element.props.className ?? "";
  const language = className.match(/language-([^\s]+)/)?.[1] ?? null;
  const code = nodeText(element.props.children).replace(/\n$/, "");

  return { code, language };
}

function nodeText(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") {
    return String(node);
  }

  if (Array.isArray(node)) {
    return node.map(nodeText).join("");
  }

  if (isValidElement(node)) {
    return nodeText((node as ReactElement<{ children?: ReactNode }>).props.children);
  }

  return "";
}
