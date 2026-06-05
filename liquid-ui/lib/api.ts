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

export type AuditedDatabase = {
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

export type CreateAuditedDatabaseRequest = {
  name: string;
  engine: "postgres";
  host: string;
  port: number;
  database: string;
  username: string;
  password: string;
  ssl_mode: "disable" | "prefer" | "require";
};

export type UpdateAuditedDatabaseRequest = Partial<
  Omit<CreateAuditedDatabaseRequest, "engine">
>;

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

async function responseError(response: Response): Promise<string> {
  try {
    const payload = (await response.json()) as { error?: string };
    return payload.error ?? `Request failed with ${response.status}`;
  } catch {
    return `Request failed with ${response.status}`;
  }
}
