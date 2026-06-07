"use client";

import { type FormEvent, type ReactNode, useEffect, useState } from "react";
import { useTheme } from "next-themes";
import {
  Eye,
  EyeOff,
  KeyRound,
  Languages,
  Loader2,
  LockKeyhole,
  Mail,
  Monitor,
  Moon,
  Sun,
  type LucideIcon,
  UserRound,
} from "lucide-react";
import { toast } from "sonner";

import { AuditDashboard } from "@/components/audit-dashboard";
import { AuthSqlTerminal } from "@/components/auth-sql-terminal";
import { ManagedDatabasePicker } from "@/components/managed-database-picker";
import { ThemedBrandImage } from "@/components/themed-brand-image";
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
type ThemeShortcut = "system" | "light" | "dark";

const themeShortcutOrder: ThemeShortcut[] = ["system", "light", "dark"];

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
  const { locale, setLocale, t } = useI18n();
  const { theme, setTheme } = useTheme();
  const [mode, setMode] = useState<AuthMode>("login");
  const [email, setEmail] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [password, setPassword] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const [isSubmitting, setIsSubmitting] = useState(false);

  const isRegister = mode === "register";
  const auth = t.auth;
  const selectedTheme: ThemeShortcut =
    theme === "light" || theme === "dark" || theme === "system"
      ? theme
      : "system";
  const nextLocale = locale === "zh-CN" ? "en-US" : "zh-CN";
  const nextLanguageLabel = nextLocale === "zh-CN" ? "中文" : "English";
  const themeShortcutLabel = t.databasePicker.themeShortcutLabel(
    t.settings.preferences.themeBadges[selectedTheme],
  );
  const languageShortcutLabel =
    t.databasePicker.languageShortcutLabel(nextLanguageLabel);

  const handleThemeShortcut = () => {
    const currentIndex = themeShortcutOrder.indexOf(selectedTheme);
    const nextTheme =
      themeShortcutOrder[(currentIndex + 1) % themeShortcutOrder.length];

    setTheme(nextTheme);
  };

  const handleLanguageShortcut = () => {
    setLocale(nextLocale);
  };

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
    <main className="flex min-h-screen items-center overflow-x-hidden bg-muted/30 p-3 text-foreground sm:p-4">
      <div className="mx-auto flex w-full min-w-0 max-w-[calc(100vw-1.5rem)] flex-col gap-5 lg:max-w-6xl">
        <div className="flex justify-center">
          <ThemedBrandImage
            src="/banner.png"
            darkSrc="/banner-dark.png"
            alt="Liquid"
            width={420}
            height={140}
            priority
            unoptimized
            draggable={false}
            className="h-auto w-full max-w-[260px] select-none object-contain sm:max-w-[360px]"
          />
          <h1 className="sr-only">Liquid SQL Audit</h1>
        </div>

        <div className="grid w-full min-w-0 gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(360px,420px)] lg:items-stretch">
          <AuthWorkbench auth={auth} />

          <Card className="flex h-full w-full min-w-0 max-w-full flex-col overflow-hidden rounded-lg py-5 shadow-xs">
            <CardHeader className="gap-4 px-5 sm:px-6">
              <div className="flex items-center justify-between gap-3">
                <div className="min-w-0 flex-1">
                  <CardTitle className="text-base">
                    {isRegister ? auth.registerTitle : auth.loginTitle}
                  </CardTitle>
                </div>
                <AuthPreferenceButtons
                  theme={selectedTheme}
                  themeLabel={themeShortcutLabel}
                  languageLabel={languageShortcutLabel}
                  onThemeClick={handleThemeShortcut}
                  onLanguageClick={handleLanguageShortcut}
                />
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
            <CardContent className="flex flex-1 px-5 sm:px-6">
              <form className="w-full space-y-4" onSubmit={handleSubmit}>
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
                  autoComplete={
                    isRegister ? "new-password" : "current-password"
                  }
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
      </div>
    </main>
  );
}

function AuthPreferenceButtons({
  theme,
  themeLabel,
  languageLabel,
  onThemeClick,
  onLanguageClick,
}: {
  theme: ThemeShortcut;
  themeLabel: string;
  languageLabel: string;
  onThemeClick: () => void;
  onLanguageClick: () => void;
}) {
  const ThemeShortcutIcon =
    theme === "dark" ? Moon : theme === "light" ? Sun : Monitor;

  return (
    <div className="flex shrink-0 items-center justify-end gap-2">
      <Button
        type="button"
        variant="outline"
        size="icon"
        className="size-8 rounded-md"
        title={themeLabel}
        aria-label={themeLabel}
        onClick={onThemeClick}
      >
        <ThemeShortcutIcon className="size-4" aria-hidden />
      </Button>
      <Button
        type="button"
        variant="outline"
        size="icon"
        className="size-8 rounded-md"
        title={languageLabel}
        aria-label={languageLabel}
        onClick={onLanguageClick}
      >
        <Languages className="size-4" aria-hidden />
      </Button>
    </div>
  );
}

function AuthWorkbench({ auth }: { auth: AuthMessages }) {
  return (
    <section className="flex h-full min-w-0 max-w-full overflow-hidden rounded-lg border bg-card text-card-foreground shadow-sm">
      <div className="flex w-full p-3 sm:p-4">
        <div className="flex w-full overflow-hidden rounded-lg border bg-background shadow-xs">
          <AuthSqlTerminal label={auth.visualAlt} />
        </div>
      </div>
    </section>
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
