export type PublicUser = {
  id: string;
  email: string;
  display_name: string;
};

export type AuthResponse = {
  token: string;
  token_type: "Bearer";
  expires_in_seconds: number;
  user: PublicUser;
};

export type ManagedDatabase = {
  id: string;
  name: string;
  engine: "postgres";
  host: string;
  port: number;
  database: string;
  username: string;
  ssl_mode: "disable" | "prefer" | "require";
  has_password: boolean;
};

export type CreateManagedDatabaseRequest = {
  name: string;
  engine: "postgres";
  host: string;
  port: number;
  database: string;
  username: string;
  password: string;
  ssl_mode: "disable" | "prefer" | "require";
};

export type UpdateManagedDatabaseRequest = Partial<
  Omit<CreateManagedDatabaseRequest, "engine">
>;

export type AgentConversation = {
  id: string;
  owner_user_id: string;
  title: string;
  created_at: string;
  updated_at: string;
};

export type AgentMessageRole = "user" | "assistant" | "tool" | "system";

export type AgentMessage = {
  id: string;
  conversation_id: string;
  turn_id?: string;
  role: AgentMessageRole;
  content: string;
  metadata?: unknown;
  created_at: string;
};

export type AgentDashboardContext = {
  active_view?: "ai" | "bi" | "databases" | "sql_audits";
  selected_sql_audit_id?: string;
  date_range?: "last_7_days" | "last_30_days";
};

export type CreateAgentTurnRequest = {
  message: string;
  managed_database_id?: string;
  dashboard_context?: AgentDashboardContext;
  client_request_id?: string;
};

export type AgentTurnStatus =
  | "queued"
  | "running"
  | "completed"
  | "blocked"
  | "failed"
  | "cancelled";

export type AgentTurn = {
  id: string;
  conversation_id: string;
  status: AgentTurnStatus;
  user_message_id: string;
  assistant_message_id?: string;
  error?: string;
  client_request_id?: string;
  managed_database_id?: string;
  dashboard_context?: AgentDashboardContext;
  created_at: string;
  updated_at: string;
  completed_at?: string;
};

export type AgentEvent = {
  seq: number;
  turn_id: string;
  type:
    | "turn_started"
    | "message_created"
    | "assistant_delta"
    | "tool_call_started"
    | "tool_call_finished"
    | "resource_created"
    | "resource_updated"
    | "action_proposed"
    | "turn_completed"
    | "turn_failed";
  payload: Record<string, unknown>;
  created_at: string;
};

export type AgentActionKind =
  | "create_sql_audit"
  | "approve_sql_audit"
  | "reject_sql_audit"
  | "execute_sql_audit"
  | "create_managed_database"
  | "update_managed_database"
  | "delete_managed_database"
  | "start_database_backup"
  | "start_database_restore";

export type AgentActionStatus =
  | "proposed"
  | "applied"
  | "rejected"
  | "failed"
  | "superseded";

export type AgentAction = {
  id: string;
  conversation_id: string;
  turn_id: string;
  kind: AgentActionKind;
  status: AgentActionStatus;
  title: string;
  description: string;
  payload: unknown;
  resource_kind?: "sql_audit" | "managed_database" | "database_backup" | "database_restore";
  resource_id?: string;
  requires_confirmation: boolean;
  created_at: string;
  updated_at: string;
};

export type AgentCapabilitiesResponse = {
  mode: string;
  capabilities: {
    name: string;
    description: string;
    requires_confirmation: boolean;
  }[];
};

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
