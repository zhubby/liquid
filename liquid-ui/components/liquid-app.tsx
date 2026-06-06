"use client";

import { type FormEvent, useEffect, useState } from "react";
import { Database, Loader2, LockKeyhole, ShieldCheck } from "lucide-react";
import { toast } from "sonner";

import { AuditDashboard } from "@/components/audit-dashboard";
import { ManagedDatabasePicker } from "@/components/managed-database-picker";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  type AuthResponse,
  type CurrentManagedDatabaseResponse,
  type ManagedDatabase,
  type PublicUser,
  apiRequest,
} from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

const TOKEN_STORAGE_KEY = "liquid.auth.token";

type AuthMode = "login" | "register";

export function LiquidApp() {
  const [token, setToken] = useState<string | null>(null);
  const [user, setUser] = useState<PublicUser | null>(null);
  const [selectedDatabase, setSelectedDatabase] =
    useState<ManagedDatabase | null>(null);
  const [checkingSession, setCheckingSession] = useState(true);

  useEffect(() => {
    const storedToken = window.localStorage.getItem(TOKEN_STORAGE_KEY);

    if (!storedToken) {
      setCheckingSession(false);
      return;
    }

    apiRequest<{ user: PublicUser }>("/api/v1/auth/me", {
      token: storedToken,
    })
      .then(async (response) => {
        const currentDatabase = await loadCurrentDatabase(storedToken);
        setToken(storedToken);
        setUser(response.user);
        setSelectedDatabase(currentDatabase);
      })
      .catch(() => {
        window.localStorage.removeItem(TOKEN_STORAGE_KEY);
      })
      .finally(() => {
        setCheckingSession(false);
      });
  }, []);

  const handleAuthenticated = async (response: AuthResponse) => {
    window.localStorage.setItem(TOKEN_STORAGE_KEY, response.token);
    const currentDatabase = await loadCurrentDatabase(response.token);
    setToken(response.token);
    setUser(response.user);
    setSelectedDatabase(currentDatabase);
  };

  const handleLogout = async () => {
    if (token) {
      try {
        await apiRequest<void>("/api/v1/auth/logout", {
          method: "POST",
          token,
        });
      } catch {
        // Local logout still wins if the server is temporarily unreachable.
      }
    }

    window.localStorage.removeItem(TOKEN_STORAGE_KEY);
    setToken(null);
    setUser(null);
    setSelectedDatabase(null);
  };

  const handleReturnToDatabasePicker = async () => {
    if (token) {
      try {
        await apiRequest<void>("/api/v1/managed-databases/current", {
          method: "DELETE",
          token,
        });
      } catch {
        // Keep the local navigation responsive if the API is briefly unavailable.
      }
    }

    setSelectedDatabase(null);
  };

  if (checkingSession) {
    return <SessionLoading />;
  }

  if (!token || !user) {
    return <AuthScreen onAuthenticated={handleAuthenticated} />;
  }

  if (!selectedDatabase) {
    return (
      <ManagedDatabasePicker
        token={token}
        user={user}
        onDatabaseSelected={setSelectedDatabase}
        onLogout={handleLogout}
        onUserUpdated={setUser}
      />
    );
  }

  return (
    <AuditDashboard
      token={token}
      user={user}
      selectedDatabase={selectedDatabase}
      onDatabaseExit={handleReturnToDatabasePicker}
    />
  );
}

async function loadCurrentDatabase(token: string): Promise<ManagedDatabase | null> {
  try {
    const response = await apiRequest<CurrentManagedDatabaseResponse>(
      "/api/v1/managed-databases/current",
      { token },
    );

    return response.database;
  } catch {
    return null;
  }
}

function SessionLoading() {
  const { t } = useI18n();

  return (
    <main className="flex min-h-screen items-center justify-center bg-muted/30 p-4 text-foreground">
      <Card className="w-full max-w-sm rounded-lg py-5 shadow-xs">
        <CardContent className="flex items-center gap-3 px-5">
          <div className="flex size-10 items-center justify-center rounded-md border bg-background">
            <Loader2 className="size-4 animate-spin text-muted-foreground" />
          </div>
          <div>
            <div className="text-sm font-medium">{t.auth.loadingTitle}</div>
            <div className="mt-1 text-xs text-muted-foreground">
              {t.auth.loadingDescription}
            </div>
          </div>
        </CardContent>
      </Card>
    </main>
  );
}

function AuthScreen({
  onAuthenticated,
}: {
  onAuthenticated: (response: AuthResponse) => void | Promise<void>;
}) {
  const { t } = useI18n();
  const [mode, setMode] = useState<AuthMode>("login");
  const [email, setEmail] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [password, setPassword] = useState("");
  const [isSubmitting, setIsSubmitting] = useState(false);

  const isRegister = mode === "register";

  const handleSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setIsSubmitting(true);

    try {
      const response = await apiRequest<AuthResponse>(
        isRegister ? "/api/v1/auth/register" : "/api/v1/auth/login",
        {
          method: "POST",
          body: isRegister
            ? {
                email,
                display_name: displayName,
                password,
              }
            : {
                email,
                password,
              },
        },
      );

      await onAuthenticated(response);
    } catch (error) {
      toast.error(error instanceof Error ? error.message : t.auth.errors.failed);
    } finally {
      setIsSubmitting(false);
    }
  };

  return (
    <main className="min-h-screen overflow-x-hidden bg-muted/30 p-3 text-foreground">
      <div className="mx-auto grid min-h-[calc(100vh-1.5rem)] w-full min-w-0 max-w-[calc(100vw-1.5rem)] gap-3 lg:max-w-5xl lg:grid-cols-[1fr_420px]">
        <section className="flex min-h-[320px] min-w-0 max-w-full flex-col justify-between overflow-hidden rounded-lg border bg-card p-5 text-card-foreground shadow-sm">
          <div>
            <div className="flex items-center gap-2">
              <div className="flex size-10 items-center justify-center rounded-md bg-primary text-primary-foreground">
                <ShieldCheck className="size-5" aria-hidden />
              </div>
              <div>
                <h1 className="text-lg font-semibold">Liquid SQL Audit</h1>
                <p className="mt-1 text-xs text-muted-foreground">
                  {t.auth.subtitle}
                </p>
              </div>
            </div>
            <div className="mt-6 grid gap-3 sm:grid-cols-3">
              <StatusItem label={t.auth.status.session} value="Bearer token" />
              <StatusItem
                label={t.auth.status.credential}
                value={t.auth.status.encryptedStorage}
              />
              <StatusItem label={t.auth.status.database} value="Postgres" />
            </div>
          </div>

          <div className="mt-6 rounded-lg border bg-background p-4">
            <div className="flex items-center gap-2 text-sm font-medium">
              <Database className="size-4 text-muted-foreground" aria-hidden />
              {t.auth.managedByLiquid}
            </div>
            <p className="mt-2 max-w-2xl text-sm leading-6 text-muted-foreground">
              {t.auth.managedDescription}
            </p>
          </div>
        </section>

        <Card className="w-full min-w-0 max-w-full self-center overflow-hidden rounded-lg py-5 shadow-xs">
          <CardHeader className="gap-3 px-5">
            <div className="flex items-center justify-between gap-3">
              <CardTitle className="text-base">
                {isRegister ? t.auth.registerTitle : t.auth.loginTitle}
              </CardTitle>
              <Badge variant="secondary" className="rounded-md">
                {isRegister ? t.auth.registerBadge : t.auth.loginBadge}
              </Badge>
            </div>
            <div className="grid grid-cols-2 rounded-md border bg-muted/40 p-1">
              <ModeButton
                active={mode === "login"}
                label={t.auth.loginTab}
                onClick={() => setMode("login")}
              />
              <ModeButton
                active={mode === "register"}
                label={t.auth.registerTab}
                onClick={() => setMode("register")}
              />
            </div>
          </CardHeader>
          <CardContent className="px-5">
            <form className="space-y-3" onSubmit={handleSubmit}>
              <Field
                id="email"
                label={t.auth.email}
                type="email"
                value={email}
                onChange={setEmail}
                autoComplete="email"
                required
              />
              {isRegister ? (
                <Field
                  id="display-name"
                  label={t.auth.displayName}
                  value={displayName}
                  onChange={setDisplayName}
                  autoComplete="name"
                  required
                />
              ) : null}
              <Field
                id="password"
                label={t.auth.password}
                type="password"
                value={password}
                onChange={setPassword}
                autoComplete={isRegister ? "new-password" : "current-password"}
                required
              />

              <Button type="submit" className="w-full" disabled={isSubmitting}>
                {isSubmitting ? (
                  <Loader2 className="size-4 animate-spin" aria-hidden />
                ) : (
                  <LockKeyhole className="size-4" aria-hidden />
                )}
                {isRegister ? t.auth.createAccount : t.auth.login}
              </Button>
            </form>
          </CardContent>
        </Card>
      </div>
    </main>
  );
}

function StatusItem({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg border bg-background p-3">
      <div className="text-xs text-muted-foreground">{label}</div>
      <div className="mt-1 text-sm font-medium">{value}</div>
    </div>
  );
}

function ModeButton({
  active,
  label,
  onClick,
}: {
  active: boolean;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      className={cn(
        "h-8 rounded-sm text-sm font-medium transition-colors",
        active
          ? "bg-background text-foreground shadow-xs"
          : "text-muted-foreground hover:text-foreground",
      )}
      onClick={onClick}
    >
      {label}
    </button>
  );
}

function Field({
  id,
  label,
  value,
  onChange,
  type = "text",
  autoComplete,
  required = false,
}: {
  id: string;
  label: string;
  value: string;
  onChange: (value: string) => void;
  type?: string;
  autoComplete?: string;
  required?: boolean;
}) {
  return (
    <div className="space-y-1.5">
      <label className="text-xs font-medium text-muted-foreground" htmlFor={id}>
        {label}
      </label>
      <input
        id={id}
        type={type}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        autoComplete={autoComplete}
        required={required}
        className="h-9 w-full rounded-md border bg-background px-3 text-sm outline-none transition-shadow placeholder:text-muted-foreground focus-visible:ring-[3px] focus-visible:ring-ring/50"
      />
    </div>
  );
}
