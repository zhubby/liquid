"use client";

import {
  Children,
  type FormEvent,
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
  Bot,
  Check,
  CheckCircle2,
  Clipboard,
  Copy,
  Database,
  Loader2,
  RotateCcw,
  Send,
  Square,
  Trash2,
  X,
} from "lucide-react";
import ReactMarkdown from "react-markdown";
import rehypeSanitize from "rehype-sanitize";
import remarkGfm from "remark-gfm";
import { toast } from "sonner";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  type AgentMessageRole,
  type ChatAction,
  type ChatConversation,
  type ChatErrorCode,
  type ChatMessage,
  type ChatMessagePart,
  type ChatStreamEvent,
  type ChatStreamStage,
  type ChatTurn,
  type LlmProviderSettingsResponse,
  type ManagedDatabase,
  apiRequest,
  apiStream,
} from "@/lib/api";
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

type ChatPanelProps = {
  token: string;
  selectedDatabase: ManagedDatabase;
  conversation: ChatConversation;
  isDeletingWorkspace: boolean;
  onConversationUpdated: (conversation: ChatConversation) => void;
  onDeleteConversation: (conversationId: string) => void | Promise<void>;
};

type CodeElementProps = {
  className?: string;
  children?: ReactNode;
};

export function ChatPanel({
  token,
  selectedDatabase,
  conversation,
  isDeletingWorkspace,
  onConversationUpdated,
  onDeleteConversation,
}: ChatPanelProps) {
  const { t } = useI18n();
  const [messages, setMessages] = useState<DisplayMessage[]>([]);
  const [actions, setActions] = useState<ChatAction[]>([]);
  const [titleInput, setTitleInput] = useState(conversation.title);
  const [input, setInput] = useState("");
  const [isLoading, setIsLoading] = useState(true);
  const [isSending, setIsSending] = useState(false);
  const [isSavingTitle, setIsSavingTitle] = useState(false);
  const [isDeleteDialogOpen, setIsDeleteDialogOpen] = useState(false);
  const [providerReady, setProviderReady] = useState<boolean | null>(null);
  const [streamStage, setStreamStage] = useState<ChatStreamStage | null>(null);
  const [activeTurn, setActiveTurn] = useState<ChatTurn | null>(null);
  const [failedTurn, setFailedTurn] = useState<FailedTurn | null>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const composerRef = useRef<HTMLTextAreaElement>(null);
  const activeStreamRef = useRef<AbortController | null>(null);
  const activeTurnRef = useRef<ChatTurn | null>(null);
  const activeConversationIdRef = useRef(conversation.id);
  const activeSendRef = useRef<string | null>(null);
  const loadVersionRef = useRef(0);
  const nearBottomRef = useRef(true);

  const actionGroups = useMemo(() => groupActionsByTurn(actions), [actions]);
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
    activeStreamRef.current = null;
    activeTurnRef.current = null;
    activeSendRef.current = null;
    nearBottomRef.current = true;
    setInput("");
    setMessages([]);
    setActions([]);
    setIsSending(false);
    setStreamStage(null);
    setActiveTurn(null);
    setFailedTurn(null);
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
      activeStreamRef.current = null;
      activeTurnRef.current = null;
      activeSendRef.current = null;
    };
  }, [conversation.id, loadConversationState, t.workspace.agentLoadFailed]);

  useEffect(() => {
    const textarea = composerRef.current;

    if (!textarea) {
      return;
    }

    textarea.style.height = "auto";
    textarea.style.height = `${Math.min(textarea.scrollHeight, 168)}px`;
  }, [input]);

  useEffect(() => {
    if (!nearBottomRef.current) {
      return;
    }

    const list = listRef.current;

    if (!list) {
      return;
    }

    list.scrollTo({ top: list.scrollHeight, behavior: "smooth" });
  }, [messages, actions, streamStage, isSending]);

  const mergeAction = useCallback((action: ChatAction) => {
    setActions((current) => {
      if (current.some((item) => item.id === action.id)) {
        return current.map((item) => (item.id === action.id ? action : item));
      }

      return [...current, action];
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
    (event: ChatStreamEvent, turn: ChatTurn, prompt: string) => {
      switch (event.type) {
        case "turn_started":
          setStreamStage("thinking");
          return false;
        case "message_created":
          setMessages((current) => upsertMessage(current, event.payload.message));
          return false;
        case "status_changed":
          setStreamStage(event.payload.stage);
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
          setMessages((current) =>
            upsertAssistantDone(current, event.payload.message, turn.id),
          );
          return false;
        case "action_proposed":
          setStreamStage("proposing_action");
          mergeAction(event.payload.action);
          return false;
        case "action_updated":
          mergeAction(event.payload.action);
          return false;
        case "turn_completed":
          setActiveTurn(event.payload.turn);
          setStreamStage(null);
          return false;
        case "turn_failed": {
          const message = chatErrorMessage(
            event.payload.error_code,
            event.payload.message,
            t,
          );

          setStreamStage(null);
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
    [mergeAction, t],
  );

  const submitPrompt = useCallback(
    async (promptOverride?: string) => {
      const content = (promptOverride ?? input).trim();

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
      setInput("");
      setIsSending(true);
      setStreamStage("thinking");
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
          setStreamStage(null);
        }
      }
    },
    [
      conversation.id,
      handleStreamEvent,
      input,
      isSending,
      selectedDatabase.id,
      t.workspace.sendFailed,
      token,
    ],
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
      setStreamStage(null);
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
  }, [activeTurn, messages, t.workspace.errorMessages.turn_cancelled, t.workspace.sendFailed, token]);

  const handleSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    void submitPrompt();
  };

  const handleComposerKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key !== "Enter" || event.shiftKey) {
      return;
    }

    event.preventDefault();
    void submitPrompt();
  };

  const handleActionDecision = async (
    action: ChatAction,
    decision: "apply" | "reject",
  ) => {
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
      toast.success(
        decision === "apply"
          ? t.workspace.actionApplied
          : t.workspace.actionRejected,
      );
    } catch (error) {
      toast.error(error instanceof Error ? error.message : t.workspace.actionFailed);
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
        providerReady={providerReady}
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
          listRef={listRef}
          messages={messages}
          actionsByTurn={actionGroups}
          orphanActions={orphanActions}
          isLoading={isLoading}
          isSending={isSending}
          streamStage={streamStage}
          selectedDatabase={selectedDatabase}
          providerReady={providerReady}
          onScroll={handleScroll}
          onPrompt={(prompt) => void submitPrompt(prompt)}
          onActionApply={(action) => void handleActionDecision(action, "apply")}
          onActionReject={(action) => void handleActionDecision(action, "reject")}
        />

        <MessageComposer
          textareaRef={composerRef}
          input={input}
          isLoading={isLoading}
          isSending={isSending}
          providerReady={providerReady}
          failedTurn={failedTurn}
          onChange={setInput}
          onSubmit={handleSubmit}
          onKeyDown={handleComposerKeyDown}
          onStop={() => void stopTurn()}
          onRetry={(prompt) => void submitPrompt(prompt)}
        />
      </div>
    </section>
  );
}

function ChatHeader({
  conversation,
  selectedDatabase,
  providerReady,
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
  providerReady: boolean | null;
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
          <ProviderBadge providerReady={providerReady} />
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

function ProviderBadge({ providerReady }: { providerReady: boolean | null }) {
  const { t } = useI18n();

  if (providerReady === null) {
    return null;
  }

  return (
    <Badge
      variant={providerReady ? "secondary" : "outline"}
      className={cn(
        "h-5 rounded-md border px-1.5 text-[11px]",
        !providerReady && "border-destructive/25 text-destructive",
      )}
    >
      {providerReady ? t.workspace.providerReady : t.workspace.providerMissing}
    </Badge>
  );
}

const MessageList = ({
  listRef,
  messages,
  actionsByTurn,
  orphanActions,
  isLoading,
  isSending,
  streamStage,
  selectedDatabase,
  providerReady,
  onScroll,
  onPrompt,
  onActionApply,
  onActionReject,
}: {
  listRef: Ref<HTMLDivElement>;
  messages: DisplayMessage[];
  actionsByTurn: Map<string, ChatAction[]>;
  orphanActions: ChatAction[];
  isLoading: boolean;
  isSending: boolean;
  streamStage: ChatStreamStage | null;
  selectedDatabase: ManagedDatabase;
  providerReady: boolean | null;
  onScroll: () => void;
  onPrompt: (prompt: string) => void;
  onActionApply: (action: ChatAction) => void;
  onActionReject: (action: ChatAction) => void;
}) => {
  const { t } = useI18n();

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
              message={message}
              actions={message.role === "assistant" ? messageActions : []}
              selectedDatabase={selectedDatabase}
              onActionApply={onActionApply}
              onActionReject={onActionReject}
            />
          );
        })}
      </div>

      {isSending || streamStage ? (
        <ChatStreamState stage={streamStage ?? "thinking"} />
      ) : null}

      {orphanActions.length > 0 ? (
        <div className="mt-4 space-y-2">
          {orphanActions.map((action) => (
            <ActionCard
              key={action.id}
              action={action}
              selectedDatabase={selectedDatabase}
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
  message,
  actions,
  selectedDatabase,
  onActionApply,
  onActionReject,
}: {
  message: DisplayMessage;
  actions: ChatAction[];
  selectedDatabase: ManagedDatabase;
  onActionApply: (action: ChatAction) => void;
  onActionReject: (action: ChatAction) => void;
}) {
  return (
    <div className="space-y-2">
      <MessageBubble message={message} />
      {actions.length > 0 ? (
        <div className="ml-11 space-y-2">
          {actions.map((action) => (
            <ActionCard
              key={action.id}
              action={action}
              selectedDatabase={selectedDatabase}
              onApply={() => onActionApply(action)}
              onReject={() => onActionReject(action)}
            />
          ))}
        </div>
      ) : null}
    </div>
  );
}

function MessageBubble({ message }: { message: DisplayMessage }) {
  const { locale, t } = useI18n();
  const isUser = message.role === "user";
  const isFailed = message.status === "failed";
  const copyText = message.content.trim();

  return (
    <article
      className={cn(
        "group flex min-w-0 gap-3",
        isUser && "justify-end text-right",
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
            "relative min-w-0 text-sm leading-6",
            isUser &&
              "rounded-lg bg-primary px-3 py-2 text-primary-foreground shadow-xs",
            !isUser && "pr-9 text-foreground",
            isFailed &&
              "rounded-md border border-destructive/25 bg-destructive/5 px-3 py-2 text-destructive",
          )}
        >
          <MessageContent message={message} />
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
          <span>{roleLabel(message.role, t)}</span>
          <span>{timeLabel(message.created_at, locale)}</span>
          {message.status === "streaming" ? (
            <span>{t.workspace.pending}</span>
          ) : null}
          {message.local ? <span>{t.workspace.localPending}</span> : null}
        </div>
      </div>
    </article>
  );
}

function MessageContent({ message }: { message: DisplayMessage }) {
  const parts =
    message.parts.length > 0
      ? message.parts
      : [{ kind: "markdown", markdown: message.content } satisfies ChatMessagePart];

  return (
    <div className="min-w-0 space-y-3">
      {parts.map((part, index) => (
        <MessagePart key={`${message.id}-${index}`} part={part} />
      ))}
    </div>
  );
}

function MessagePart({ part }: { part: ChatMessagePart }) {
  const { t } = useI18n();

  switch (part.kind) {
    case "text":
      return <p className="whitespace-pre-wrap break-words">{part.text}</p>;
    case "markdown":
      return <MarkdownContent markdown={part.markdown} />;
    case "code":
      return <CodeBlock code={part.code} language={part.language} />;
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
      return <ChatStreamState stage={part.stage} compact />;
    case "action_ref":
      return null;
  }
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

  return (
    <div className="overflow-hidden rounded-md border bg-muted/60 text-left">
      <div className="flex h-8 items-center justify-between border-b bg-muted px-3">
        <span className="text-[11px] font-medium uppercase text-muted-foreground">
          {label}
        </span>
        <CopyButton text={code} title={t.workspace.copyCode} compact />
      </div>
      <pre className="max-h-72 overflow-auto p-3 text-xs leading-5">
        <code className="font-mono">{code}</code>
      </pre>
    </div>
  );
}

function ActionCard({
  action,
  selectedDatabase,
  onApply,
  onReject,
}: {
  action: ChatAction;
  selectedDatabase: ManagedDatabase;
  onApply: () => void;
  onReject: () => void;
}) {
  const { t } = useI18n();
  const isProposed = action.status === "proposed";
  const databaseName =
    action.preview?.kind === "sql_audit"
      ? action.preview.database_name ?? selectedDatabase.name
      : selectedDatabase.name;

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
        <Badge
          variant={isProposed ? "secondary" : "outline"}
          className="rounded-md"
        >
          {t.workspace.actionStatuses[action.status]}
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

      <div className="mt-3 flex flex-wrap gap-2">
        {isProposed ? (
          <>
            <Button type="button" size="sm" onClick={onApply}>
              <CheckCircle2 className="size-4" aria-hidden />
              {t.workspace.confirm}
            </Button>
            <Button type="button" variant="outline" size="sm" onClick={onReject}>
              <X className="size-4" aria-hidden />
              {t.workspace.reject}
            </Button>
          </>
        ) : (
          <div className="flex items-center gap-2 text-xs text-muted-foreground">
            <Check className="size-3.5" aria-hidden />
            {t.workspace.actionStatusUpdated}
          </div>
        )}
      </div>
    </article>
  );
}

function ChatStreamState({
  stage,
  compact = false,
}: {
  stage: ChatStreamStage;
  compact?: boolean;
}) {
  const { t } = useI18n();

  return (
    <div
      className={cn(
        "flex items-center gap-2 text-sm text-muted-foreground",
        compact ? "py-1" : "mt-4 pl-11",
      )}
    >
      <span className="relative flex size-4">
        <span className="absolute inline-flex size-full animate-ping rounded-full bg-primary/25" />
        <span className="relative inline-flex size-4 rounded-full border border-primary/30 bg-primary/15" />
      </span>
      <span>{t.workspace.stages[stage]}</span>
    </div>
  );
}

const MessageComposer = ({
  textareaRef,
  input,
  isLoading,
  isSending,
  providerReady,
  failedTurn,
  onChange,
  onSubmit,
  onKeyDown,
  onStop,
  onRetry,
}: {
  textareaRef: Ref<HTMLTextAreaElement>;
  input: string;
  isLoading: boolean;
  isSending: boolean;
  providerReady: boolean | null;
  failedTurn: FailedTurn | null;
  onChange: (value: string) => void;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
  onKeyDown: (event: KeyboardEvent<HTMLTextAreaElement>) => void;
  onStop: () => void;
  onRetry: (prompt: string) => void;
}) => {
  const { t } = useI18n();
  const canSubmit = Boolean(input.trim()) && !isLoading && !isSending;

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
              onClick={() => onRetry(failedTurn.prompt)}
            >
              <RotateCcw className="size-4" aria-hidden />
              {t.workspace.retry}
            </Button>
          ) : null}
        </div>
      ) : null}

      {providerReady === false ? (
        <div className="mb-3 flex items-center gap-2 text-xs text-muted-foreground">
          <AlertTriangle className="size-3.5 text-destructive" aria-hidden />
          <span>{t.workspace.providerSetupHint}</span>
        </div>
      ) : null}

      <form
        className="rounded-lg border bg-background p-2 shadow-xs focus-within:ring-[3px] focus-within:ring-ring/50"
        onSubmit={onSubmit}
      >
        <label className="sr-only" htmlFor="ai-message">
          {t.workspace.inputLabel}
        </label>
        <textarea
          ref={textareaRef}
          id="ai-message"
          className="max-h-[10.5rem] min-h-12 w-full resize-none bg-transparent px-2 py-1.5 text-sm leading-6 outline-none placeholder:text-muted-foreground"
          placeholder={t.workspace.inputPlaceholder}
          value={input}
          disabled={isLoading}
          rows={1}
          onChange={(event) => onChange(event.target.value)}
          onKeyDown={onKeyDown}
        />
        <div className="mt-2 flex items-center justify-between gap-2">
          <span className="px-2 text-xs text-muted-foreground">
            {isSending ? t.workspace.pending : t.workspace.composerHint}
          </span>
          {isSending ? (
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
          ) : (
            <Button
              type="submit"
              size="sm"
              className="h-8 rounded-md"
              aria-label={t.workspace.sendQuestion}
              title={t.workspace.send}
              disabled={!canSubmit}
            >
              <Send className="size-4" aria-hidden />
              {t.workspace.send}
            </Button>
          )}
        </div>
      </form>
    </footer>
  );
};

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

  return role;
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
