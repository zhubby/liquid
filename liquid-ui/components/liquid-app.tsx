"use client";

import { type FormEvent, type ReactNode, useEffect, useState } from "react";
import {
  Activity,
  CheckCircle2,
  Database,
  Eye,
  EyeOff,
  KeyRound,
  Loader2,
  LockKeyhole,
  Mail,
  Server,
  ShieldCheck,
  TerminalSquare,
  type LucideIcon,
  UserRound,
} from "lucide-react";
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
type AuthMessages = ReturnType<typeof useI18n>["t"]["auth"];

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
  const [showPassword, setShowPassword] = useState(false);
  const [isSubmitting, setIsSubmitting] = useState(false);

  const isRegister = mode === "register";
  const auth = t.auth;

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
    <main className="min-h-screen overflow-x-hidden bg-muted/30 p-3 text-foreground sm:p-4">
      <div className="mx-auto grid w-full min-w-0 max-w-[calc(100vw-1.5rem)] gap-4 lg:min-h-[calc(100vh-2rem)] lg:max-w-6xl lg:grid-cols-[minmax(0,1fr)_minmax(360px,420px)] lg:items-center">
        <AuthWorkbench auth={auth} />

        <Card className="w-full min-w-0 max-w-full self-start overflow-hidden rounded-lg py-5 shadow-xs lg:self-center">
          <CardHeader className="gap-4 px-5 sm:px-6">
            <div className="flex items-center justify-between gap-3">
              <div className="min-w-0">
                <CardTitle className="text-base">
                  {isRegister ? auth.registerTitle : auth.loginTitle}
                </CardTitle>
                <p className="mt-1 text-sm leading-5 text-muted-foreground">
                  {isRegister
                    ? auth.registerDescription
                    : auth.loginDescription}
                </p>
              </div>
              <Badge variant="secondary" className="rounded-md">
                {isRegister ? auth.registerBadge : auth.loginBadge}
              </Badge>
            </div>
            <div className="grid grid-cols-2 rounded-md border bg-muted/40 p-1">
              <ModeButton
                active={mode === "login"}
                disabled={isSubmitting}
                label={auth.loginTab}
                onClick={() => setMode("login")}
              />
              <ModeButton
                active={mode === "register"}
                disabled={isSubmitting}
                label={auth.registerTab}
                onClick={() => setMode("register")}
              />
            </div>
          </CardHeader>
          <CardContent className="px-5 sm:px-6">
            <form className="space-y-4" onSubmit={handleSubmit}>
              <Field
                id="email"
                autoComplete="email"
                disabled={isSubmitting}
                icon={Mail}
                label={auth.email}
                placeholder={auth.emailPlaceholder}
                required
                type="email"
                value={email}
                onChange={setEmail}
              />
              {isRegister ? (
                <Field
                  id="display-name"
                  autoComplete="name"
                  disabled={isSubmitting}
                  icon={UserRound}
                  label={auth.displayName}
                  placeholder={auth.displayNamePlaceholder}
                  required
                  value={displayName}
                  onChange={setDisplayName}
                />
              ) : null}
              <Field
                id="password"
                autoComplete={isRegister ? "new-password" : "current-password"}
                disabled={isSubmitting}
                icon={KeyRound}
                label={auth.password}
                placeholder={auth.passwordPlaceholder}
                value={password}
                onChange={setPassword}
                type={showPassword ? "text" : "password"}
                required
                trailing={
                  <button
                    type="button"
                    aria-label={
                      showPassword ? auth.hidePassword : auth.showPassword
                    }
                    className="absolute right-1.5 top-1/2 flex size-7 -translate-y-1/2 items-center justify-center rounded-md text-muted-foreground outline-none transition-colors hover:bg-accent hover:text-foreground focus-visible:ring-[3px] focus-visible:ring-ring/50 disabled:pointer-events-none disabled:opacity-50"
                    disabled={isSubmitting}
                    onClick={() => setShowPassword((visible) => !visible)}
                  >
                    {showPassword ? (
                      <EyeOff className="size-4" aria-hidden />
                    ) : (
                      <Eye className="size-4" aria-hidden />
                    )}
                  </button>
                }
              />

              <Button
                type="submit"
                className="h-10 w-full"
                disabled={isSubmitting}
              >
                {isSubmitting ? (
                  <Loader2 className="size-4 animate-spin" aria-hidden />
                ) : (
                  <LockKeyhole className="size-4" aria-hidden />
                )}
                {isRegister ? auth.createAccount : auth.login}
              </Button>
            </form>
          </CardContent>
        </Card>
      </div>
    </main>
  );
}

function AuthWorkbench({ auth }: { auth: AuthMessages }) {
  return (
    <section className="min-w-0 max-w-full overflow-hidden rounded-lg border bg-card text-card-foreground shadow-sm">
      <div className="border-b p-5 sm:p-6">
        <div className="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
          <div className="flex min-w-0 items-start gap-3">
            <div className="flex size-11 shrink-0 items-center justify-center rounded-lg bg-primary text-primary-foreground shadow-sm">
              <ShieldCheck className="size-5" aria-hidden />
            </div>
            <div className="min-w-0">
              <Badge variant="outline" className="mb-2 rounded-md">
                {auth.workspaceBadge}
              </Badge>
              <h1 className="text-balance text-xl font-semibold tracking-normal sm:text-2xl">
                Liquid SQL Audit
              </h1>
              <p className="mt-2 max-w-2xl text-sm leading-6 text-muted-foreground">
                {auth.subtitle}
              </p>
            </div>
          </div>
        </div>
      </div>

      <div className="hidden gap-4 p-5 sm:p-6 md:grid xl:grid-cols-[minmax(0,0.85fr)_minmax(0,1.15fr)]">
        <div className="min-w-0 space-y-3">
          <StatusItem
            icon={ShieldCheck}
            label={auth.status.session}
            value={auth.status.tokenSession}
          />
          <StatusItem
            icon={LockKeyhole}
            label={auth.status.credential}
            value={auth.status.encryptedStorage}
          />
          <StatusItem
            icon={Database}
            label={auth.status.database}
            value={auth.status.postgresConnections}
          />

          <div className="min-w-0 rounded-lg border bg-background p-4">
            <div className="flex items-center gap-2 text-sm font-medium">
              <Server className="size-4 text-muted-foreground" aria-hidden />
              {auth.managedByLiquid}
            </div>
            <p className="mt-2 text-sm leading-6 text-muted-foreground">
              {auth.managedDescription}
            </p>
          </div>
        </div>

        <div className="min-w-0 overflow-hidden rounded-lg border bg-background shadow-xs">
          <div className="flex flex-col gap-2 border-b p-4 sm:flex-row sm:items-start sm:justify-between">
            <div className="min-w-0">
              <div className="flex items-center gap-2 text-sm font-medium">
                <TerminalSquare
                  className="size-4 text-muted-foreground"
                  aria-hidden
                />
                {auth.preview.title}
              </div>
              <p className="mt-1 text-xs leading-5 text-muted-foreground">
                {auth.preview.description}
              </p>
            </div>
            <Badge variant="secondary" className="shrink-0 rounded-md">
              {auth.preview.badge}
            </Badge>
          </div>

          <div className="space-y-4 p-4">
            <div className="max-w-full overflow-hidden rounded-md border bg-muted/40">
              <div className="flex items-center justify-between border-b px-3 py-2">
                <span className="text-xs font-medium text-muted-foreground">
                  {auth.preview.queryLabel}
                </span>
                <RiskPill label={auth.preview.reviewRequired} />
              </div>
              <pre className="overflow-x-auto p-3 text-xs leading-6 text-foreground">
                <code>{`update invoices
set status = 'void'
where approved_by is null;`}</code>
              </pre>
            </div>

            <div className="grid gap-2">
              <RiskRow
                label={auth.preview.policyFinding}
                status={auth.preview.requiresReview}
                tone="critical"
              />
              <RiskRow
                label={auth.preview.credentialFinding}
                status={auth.preview.protected}
                tone="watch"
              />
              <RiskRow
                label={auth.preview.databaseFinding}
                status={auth.preview.ready}
                tone="ok"
              />
            </div>

            <div className="grid gap-2 rounded-md border bg-card p-3">
              <div className="flex items-center gap-2 text-xs font-medium text-muted-foreground">
                <Activity className="size-3.5" aria-hidden />
                {auth.preview.flowTitle}
              </div>
              <div className="grid gap-2 sm:grid-cols-3">
                <FlowStep label={auth.preview.flowConnect} />
                <FlowStep label={auth.preview.flowAudit} />
                <FlowStep label={auth.preview.flowGate} />
              </div>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}

function StatusItem({
  icon: Icon,
  label,
  value,
}: {
  icon: LucideIcon;
  label: string;
  value: string;
}) {
  return (
    <div className="flex items-center gap-3 rounded-lg border bg-background p-3 shadow-xs">
      <div className="flex size-9 shrink-0 items-center justify-center rounded-md border bg-muted/40 text-muted-foreground">
        <Icon className="size-4" aria-hidden />
      </div>
      <div className="min-w-0">
        <div className="text-xs text-muted-foreground">{label}</div>
        <div className="mt-1 truncate text-sm font-medium">{value}</div>
      </div>
    </div>
  );
}

function RiskPill({ label }: { label: string }) {
  return (
    <span className="inline-flex shrink-0 items-center gap-1.5 rounded-md border border-destructive/30 bg-destructive/10 px-2 py-0.5 text-xs font-medium text-destructive">
      <span className="size-1.5 rounded-full bg-destructive" />
      {label}
    </span>
  );
}

function RiskRow({
  label,
  status,
  tone,
}: {
  label: string;
  status: string;
  tone: "critical" | "watch" | "ok";
}) {
  const markerColor =
    tone === "critical"
      ? "var(--destructive)"
      : tone === "watch"
        ? "var(--chart-4)"
        : "var(--chart-2)";

  return (
    <div className="flex min-w-0 items-center justify-between gap-3 rounded-md border bg-card px-3 py-2 text-sm">
      <div className="flex min-w-0 items-center gap-2">
        <span
          className="size-2 shrink-0 rounded-full"
          style={{ backgroundColor: markerColor }}
        />
        <span className="truncate">{label}</span>
      </div>
      <span className="shrink-0 text-xs text-muted-foreground">{status}</span>
    </div>
  );
}

function FlowStep({ label }: { label: string }) {
  return (
    <div className="flex items-center gap-1.5 rounded-md bg-muted/50 px-2 py-1.5 text-xs font-medium">
      <CheckCircle2
        className="size-3.5 shrink-0"
        style={{ color: "var(--chart-2)" }}
        aria-hidden
      />
      <span className="min-w-0 truncate">{label}</span>
    </div>
  );
}

function ModeButton({
  active,
  disabled = false,
  label,
  onClick,
}: {
  active: boolean;
  disabled?: boolean;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      className={cn(
        "h-8 rounded-sm text-sm font-medium outline-none transition-colors focus-visible:ring-[3px] focus-visible:ring-ring/50 disabled:pointer-events-none disabled:opacity-50",
        active
          ? "bg-background text-foreground shadow-xs"
          : "text-muted-foreground hover:text-foreground",
      )}
      aria-pressed={active}
      disabled={disabled}
      onClick={onClick}
    >
      {label}
    </button>
  );
}

function Field({
  id,
  label,
  icon: Icon,
  placeholder,
  value,
  onChange,
  type = "text",
  autoComplete,
  disabled = false,
  required = false,
  trailing,
}: {
  id: string;
  label: string;
  icon: LucideIcon;
  placeholder?: string;
  value: string;
  onChange: (value: string) => void;
  type?: string;
  autoComplete?: string;
  disabled?: boolean;
  required?: boolean;
  trailing?: ReactNode;
}) {
  return (
    <div className="space-y-1.5">
      <label className="text-xs font-medium text-muted-foreground" htmlFor={id}>
        {label}
      </label>
      <div className="relative">
        <Icon
          className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground"
          aria-hidden
        />
        <input
          id={id}
          type={type}
          value={value}
          onChange={(event) => onChange(event.target.value)}
          autoComplete={autoComplete}
          disabled={disabled}
          placeholder={placeholder}
          required={required}
          className={cn(
            "h-10 w-full rounded-md border bg-background py-2 pl-9 text-sm shadow-xs outline-none transition-shadow placeholder:text-muted-foreground focus-visible:ring-[3px] focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-50",
            trailing ? "pr-10" : "pr-3",
          )}
        />
        {trailing}
      </div>
    </div>
  );
}
