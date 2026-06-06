export type { AgentAction } from "./generated/api-types/AgentAction";
export type { AgentActionKind } from "./generated/api-types/AgentActionKind";
export type { AgentActionStatus } from "./generated/api-types/AgentActionStatus";
export type { AgentActiveView } from "./generated/api-types/AgentActiveView";
export type { AgentDateRange } from "./generated/api-types/AgentDateRange";
export type { AgentEventType } from "./generated/api-types/AgentEventType";
export type { AgentMessage } from "./generated/api-types/AgentMessage";
export type { AgentMessageRole } from "./generated/api-types/AgentMessageRole";
export type { AgentResourceKind } from "./generated/api-types/AgentResourceKind";
export type { AgentTurn } from "./generated/api-types/AgentTurn";
export type { AgentTurnStatus } from "./generated/api-types/AgentTurnStatus";
export type { ApproveSqlAuditRequest } from "./generated/api-types/ApproveSqlAuditRequest";
export type { AuditSummary } from "./generated/api-types/AuditSummary";
export type { AuditTrendPoint } from "./generated/api-types/AuditTrendPoint";
export type { AuthResponse } from "./generated/api-types/AuthResponse";
export type { ChatAction } from "./generated/api-types/ChatAction";
export type { ChatActionDecisionRequest } from "./generated/api-types/ChatActionDecisionRequest";
export type { ChatActionPreview } from "./generated/api-types/ChatActionPreview";
export type { ChatConversation } from "./generated/api-types/ChatConversation";
export type { ChatErrorCode } from "./generated/api-types/ChatErrorCode";
export type { ChatManagedDatabaseSummary } from "./generated/api-types/ChatManagedDatabaseSummary";
export type { ChatMessage } from "./generated/api-types/ChatMessage";
export type { ChatMessagePart } from "./generated/api-types/ChatMessagePart";
export type { ChatMessageStatus } from "./generated/api-types/ChatMessageStatus";
export type { ChatStreamEvent } from "./generated/api-types/ChatStreamEvent";
export type { ChatStreamStage } from "./generated/api-types/ChatStreamStage";
export type { ChatTurn } from "./generated/api-types/ChatTurn";
export type { ChatTurnDashboardContext } from "./generated/api-types/ChatTurnDashboardContext";
export type { CreateAgentActionRequest } from "./generated/api-types/CreateAgentActionRequest";
export type { CreateChatConversationRequest } from "./generated/api-types/CreateChatConversationRequest";
export type { CreateChatTurnRequest } from "./generated/api-types/CreateChatTurnRequest";
export type { CreateManagedDatabaseRequest } from "./generated/api-types/CreateManagedDatabaseRequest";
export type { CreateSqlAuditRequest } from "./generated/api-types/CreateSqlAuditRequest";
export type { CurrentManagedDatabaseResponse } from "./generated/api-types/CurrentManagedDatabaseResponse";
export type { CurrentUserResponse } from "./generated/api-types/CurrentUserResponse";
export type { DatabaseBackupFormat } from "./generated/api-types/DatabaseBackupFormat";
export type { DatabaseBackupObjectMetadata } from "./generated/api-types/DatabaseBackupObjectMetadata";
export type { DatabaseBackupRecord } from "./generated/api-types/DatabaseBackupRecord";
export type { DatabaseBackupStatus } from "./generated/api-types/DatabaseBackupStatus";
export type { DatabaseRestoreRecord } from "./generated/api-types/DatabaseRestoreRecord";
export type { LoginRequest } from "./generated/api-types/LoginRequest";
export type { LlmProviderApiMode } from "./generated/api-types/LlmProviderApiMode";
export type { LlmProviderKind } from "./generated/api-types/LlmProviderKind";
export type { LlmProviderSettings } from "./generated/api-types/LlmProviderSettings";
export type { LlmProviderSettingsResponse } from "./generated/api-types/LlmProviderSettingsResponse";
export type { ManagedDatabase } from "./generated/api-types/ManagedDatabase";
export type { ManagedDatabaseConnectionTestResponse } from "./generated/api-types/ManagedDatabaseConnectionTestResponse";
export type { ManagedDatabaseEngine } from "./generated/api-types/ManagedDatabaseEngine";
export type { ManagedDatabaseSnapshot } from "./generated/api-types/ManagedDatabaseSnapshot";
export type { ManagedDatabaseSslMode } from "./generated/api-types/ManagedDatabaseSslMode";
export type { PublicUser } from "./generated/api-types/PublicUser";
export type { RegisterRequest } from "./generated/api-types/RegisterRequest";
export type { RejectSqlAuditRequest } from "./generated/api-types/RejectSqlAuditRequest";
export type { RiskBreakdown } from "./generated/api-types/RiskBreakdown";
export type { RiskSeverity } from "./generated/api-types/RiskSeverity";
export type { SetCurrentManagedDatabaseRequest } from "./generated/api-types/SetCurrentManagedDatabaseRequest";
export type { SqlAuditExecutionResult } from "./generated/api-types/SqlAuditExecutionResult";
export type { SqlAuditFinding } from "./generated/api-types/SqlAuditFinding";
export type { SqlAuditRecord } from "./generated/api-types/SqlAuditRecord";
export type { SqlAuditReport } from "./generated/api-types/SqlAuditReport";
export type { SqlAuditRequest } from "./generated/api-types/SqlAuditRequest";
export type { SqlAuditStatus } from "./generated/api-types/SqlAuditStatus";
export type { SqlStatementKind } from "./generated/api-types/SqlStatementKind";
export type { UpdateChatConversationRequest } from "./generated/api-types/UpdateChatConversationRequest";
export type { UpdateCurrentUserRequest } from "./generated/api-types/UpdateCurrentUserRequest";
export type { UpdateLlmProviderSettingsRequest } from "./generated/api-types/UpdateLlmProviderSettingsRequest";
export type { UpdateManagedDatabaseRequest } from "./generated/api-types/UpdateManagedDatabaseRequest";
export type { UpdatePasswordRequest } from "./generated/api-types/UpdatePasswordRequest";

export class ApiError extends Error {
  status: number;

  constructor(message: string, status: number) {
    super(message);
    this.name = "ApiError";
    this.status = status;
  }
}

const API_BASE_URL =
  process.env.NEXT_PUBLIC_API_BASE_URL ?? "http://localhost:3001";

export async function apiRequest<T>(
  path: string,
  options: {
    method?: string;
    token?: string;
    body?: unknown;
  } = {},
): Promise<T> {
  const headers = new Headers();

  if (options.body !== undefined) {
    headers.set("Content-Type", "application/json");
  }

  if (options.token) {
    headers.set("Authorization", `Bearer ${options.token}`);
  }

  const response = await fetch(`${API_BASE_URL}${path}`, {
    method: options.method ?? "GET",
    headers,
    body: options.body === undefined ? undefined : JSON.stringify(options.body),
  });

  if (!response.ok) {
    throw new ApiError(await responseError(response), response.status);
  }

  if (response.status === 204) {
    return undefined as T;
  }

  return response.json() as Promise<T>;
}

export async function apiStream<T>(
  path: string,
  options: {
    token?: string;
    signal?: AbortSignal;
    onEvent: (event: T, eventName: string) => void;
  },
): Promise<void> {
  const headers = new Headers();

  if (options.token) {
    headers.set("Authorization", `Bearer ${options.token}`);
  }

  const response = await fetch(`${API_BASE_URL}${path}`, {
    headers,
    signal: options.signal,
  });

  if (!response.ok) {
    throw new ApiError(await responseError(response), response.status);
  }

  if (!response.body) {
    return;
  }

  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";

  while (true) {
    const { done, value } = await reader.read();

    if (done) {
      break;
    }

    buffer += decoder.decode(value, { stream: true });
    const frames = buffer.split(/\r?\n\r?\n/);
    buffer = frames.pop() ?? "";

    for (const frame of frames) {
      const parsed = parseSseFrame(frame);

      if (!parsed || parsed.eventName === "ping") {
        continue;
      }

      options.onEvent(JSON.parse(parsed.data) as T, parsed.eventName);
    }
  }
}

async function responseError(response: Response): Promise<string> {
  try {
    const payload = (await response.json()) as { error?: string };
    return payload.error ?? `Request failed with ${response.status}`;
  } catch {
    return `Request failed with ${response.status}`;
  }
}

function parseSseFrame(
  frame: string,
): { eventName: string; data: string } | null {
  let eventName = "message";
  const data: string[] = [];

  for (const line of frame.split(/\r?\n/)) {
    if (line.startsWith("event:")) {
      eventName = line.slice("event:".length).trim();
    } else if (line.startsWith("data:")) {
      data.push(line.slice("data:".length).trimStart());
    }
  }

  if (data.length === 0) {
    return null;
  }

  return {
    eventName,
    data: data.join("\n"),
  };
}
