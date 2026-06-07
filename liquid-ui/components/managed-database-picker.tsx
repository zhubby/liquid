"use client";

import {
  type FormEvent,
  type ReactNode,
  type KeyboardEvent as ReactKeyboardEvent,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import Image from "next/image";
import { useTheme } from "next-themes";
import {
  Archive,
  ChevronDown,
  CircleAlert,
  CircleCheck,
  ClipboardCheck,
  Database,
  KeyRound,
  Languages,
  Loader2,
  LogIn,
  LogOut,
  Monitor,
  Moon,
  PencilLine,
  Plug,
  Plus,
  Save,
  Search,
  Server,
  Settings,
  Sun,
  Tags,
  Trash2,
  X,
} from "lucide-react";
import { toast } from "sonner";

import { AccountSettingsDialog } from "@/components/account-settings-dialog";
import { SqlAuditHistory } from "@/components/sql-audit-history";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  type CreateManagedDatabaseRequest,
  type CurrentManagedDatabaseResponse,
  type ManagedDatabase,
  type ManagedDatabaseConnectionTestResponse,
  type PublicUser,
  type SetCurrentManagedDatabaseRequest,
  type UpdateManagedDatabaseRequest,
  apiRequest,
} from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

type ManagedDatabasePickerProps = {
  token: string;
  user: PublicUser;
  onDatabaseSelected: (database: ManagedDatabase) => void;
  onLogout: () => void;
  onUserUpdated: (user: PublicUser) => void;
};

type ManagedDatabaseForm = {
  name: string;
  host: string;
  port: string;
  database: string;
  username: string;
  password: string;
  tags: string[];
  ssl_mode: ManagedDatabase["ssl_mode"];
};

type ConnectionTestState = {
  tone: "success" | "error";
  message: string;
};

type ManagedDatabaseView = "overview" | "audits" | "backups";

const emptyManagedDatabaseForm: ManagedDatabaseForm = {
  name: "",
  host: "",
  port: "5432",
  database: "",
  username: "",
  password: "",
  tags: [],
  ssl_mode: "prefer",
};

type ThemeShortcut = "system" | "light" | "dark";

const themeShortcutOrder: ThemeShortcut[] = ["system", "light", "dark"];

export function ManagedDatabasePicker({
  token,
  user,
  onDatabaseSelected,
  onLogout,
  onUserUpdated,
}: ManagedDatabasePickerProps) {
  const { locale, setLocale, t } = useI18n();
  const { theme, setTheme } = useTheme();
  const [databases, setDatabases] = useState<ManagedDatabase[]>([]);
  const [query, setQuery] = useState("");
  const [form, setForm] = useState<ManagedDatabaseForm>(
    emptyManagedDatabaseForm,
  );
  const [editingId, setEditingId] = useState<string | null>(null);
  const [isFormOpen, setIsFormOpen] = useState(false);
  const [isLoading, setIsLoading] = useState(true);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [testingId, setTestingId] = useState<string | null>(null);
  const [enteringId, setEnteringId] = useState<string | null>(null);
  const [deletingId, setDeletingId] = useState<string | null>(null);
  const [formError, setFormError] = useState<string | null>(null);
  const [isAccountMenuOpen, setIsAccountMenuOpen] = useState(false);
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [activeView, setActiveView] = useState<ManagedDatabaseView>("overview");
  const accountMenuRef = useRef<HTMLDivElement | null>(null);
  const [testResults, setTestResults] = useState<
    Record<string, ConnectionTestState>
  >({});

  const selectedTheme: ThemeShortcut =
    theme === "light" || theme === "dark" || theme === "system"
      ? theme
      : "system";
  const ThemeShortcutIcon =
    selectedTheme === "dark" ? Moon : selectedTheme === "light" ? Sun : Monitor;
  const nextLocale = locale === "zh-CN" ? "en-US" : "zh-CN";
  const nextLanguageLabel = nextLocale === "zh-CN" ? "中文" : "English";
  const themeShortcutLabel = t.databasePicker.themeShortcutLabel(
    t.settings.preferences.themeBadges[selectedTheme],
  );
  const languageShortcutLabel =
    t.databasePicker.languageShortcutLabel(nextLanguageLabel);

  const loadDatabases = useCallback(async () => {
    setIsLoading(true);

    try {
      const response = await apiRequest<ManagedDatabase[]>(
        "/api/v1/managed-databases",
        { token },
      );
      setDatabases(response);
    } catch (error) {
      toast.error(
        error instanceof Error ? error.message : t.databasePicker.loadFailed,
      );
    } finally {
      setIsLoading(false);
    }
  }, [t.databasePicker.loadFailed, token]);

  useEffect(() => {
    void loadDatabases();
  }, [loadDatabases]);

  useEffect(() => {
    if (!isAccountMenuOpen) {
      return;
    }

    const handlePointerDown = (event: PointerEvent) => {
      if (!accountMenuRef.current?.contains(event.target as Node)) {
        setIsAccountMenuOpen(false);
      }
    };

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setIsAccountMenuOpen(false);
      }
    };

    document.addEventListener("pointerdown", handlePointerDown);
    document.addEventListener("keydown", handleKeyDown);

    return () => {
      document.removeEventListener("pointerdown", handlePointerDown);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [isAccountMenuOpen]);

  const filteredDatabases = useMemo(() => {
    const term = query.trim().toLowerCase();

    if (!term) {
      return databases;
    }

    return databases.filter((database) =>
      [
        database.name,
        database.host,
        database.database,
        database.username,
        database.ssl_mode,
        ...(database.tags ?? []),
      ]
        .join(" ")
        .toLowerCase()
        .includes(term),
    );
  }, [databases, query]);

  const userInitials =
    user.display_name
      .split(/\s+/)
      .filter(Boolean)
      .slice(0, 2)
      .map((part) => part[0])
      .join("")
      .toUpperCase() || user.email.slice(0, 1).toUpperCase();
  const menuItems = [
    {
      value: "overview" as const,
      icon: Database,
      label: t.databasePicker.databaseOverview,
      description: t.databasePicker.connectionCount(databases.length),
      badge: String(databases.length),
    },
    {
      value: "audits" as const,
      icon: ClipboardCheck,
      label: t.databasePicker.auditMenu,
      description: t.databasePicker.auditMenuDescription,
      badge: null,
    },
    {
      value: "backups" as const,
      icon: Archive,
      label: t.databasePicker.backupMenu,
      description: t.databasePicker.backupMenuDescription,
      badge: null,
    },
  ];

  const resetForm = () => {
    setForm(emptyManagedDatabaseForm);
    setEditingId(null);
  };

  const openCreateForm = () => {
    resetForm();
    setFormError(null);
    setIsFormOpen(true);
  };

  const closeForm = () => {
    setIsFormOpen(false);
    setFormError(null);
    resetForm();
  };

  const openSettings = () => {
    setIsAccountMenuOpen(false);
    setIsSettingsOpen(true);
  };

  const handleThemeShortcut = () => {
    const currentIndex = themeShortcutOrder.indexOf(selectedTheme);
    const nextTheme =
      themeShortcutOrder[(currentIndex + 1) % themeShortcutOrder.length];

    setTheme(nextTheme);
  };

  const handleLanguageShortcut = () => {
    setLocale(nextLocale);
  };

  const handleLogout = () => {
    setIsAccountMenuOpen(false);
    onLogout();
  };

  const handleSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setFormError(null);
    setIsSubmitting(true);

    const port = Number.parseInt(form.port, 10);

    if (!Number.isInteger(port) || port < 1 || port > 65_535) {
      setFormError(t.databasePicker.portRangeError);
      setIsSubmitting(false);
      return;
    }

    try {
      if (editingId) {
        const body: UpdateManagedDatabaseRequest = {
          name: form.name,
          host: form.host,
          port,
          database: form.database,
          username: form.username,
          tags: form.tags,
          ssl_mode: form.ssl_mode,
        };

        if (form.password.trim()) {
          body.password = form.password;
        }

        const updated = await apiRequest<ManagedDatabase>(
          `/api/v1/managed-databases/${editingId}`,
          {
            method: "PATCH",
            token,
            body,
          },
        );

        setDatabases((current) =>
          current.map((database) =>
            database.id === updated.id ? updated : database,
          ),
        );
        toast.success(t.databasePicker.updated);
      } else {
        const body: CreateManagedDatabaseRequest = {
          name: form.name,
          engine: "postgres",
          host: form.host,
          port,
          database: form.database,
          username: form.username,
          password: form.password,
          tags: form.tags,
          ssl_mode: form.ssl_mode,
        };
        const created = await apiRequest<ManagedDatabase>(
          "/api/v1/managed-databases",
          {
            method: "POST",
            token,
            body,
          },
        );

        setDatabases((current) => [...current, created]);
        toast.success(t.databasePicker.created);
      }

      closeForm();
    } catch (error) {
      toast.error(
        error instanceof Error ? error.message : t.databasePicker.saveFailed,
      );
    } finally {
      setIsSubmitting(false);
    }
  };

  const handleEdit = (database: ManagedDatabase) => {
    setEditingId(database.id);
    setForm({
      name: database.name,
      host: database.host,
      port: String(database.port),
      database: database.database,
      username: database.username,
      password: "",
      tags: database.tags ?? [],
      ssl_mode: database.ssl_mode,
    });
    setFormError(null);
    setIsFormOpen(true);
  };

  const handleDelete = async (database: ManagedDatabase) => {
    const confirmed = window.confirm(
      t.databasePicker.deleteConfirm(database.name),
    );

    if (!confirmed) {
      return;
    }

    setDeletingId(database.id);

    try {
      await apiRequest<void>(`/api/v1/managed-databases/${database.id}`, {
        method: "DELETE",
        token,
      });
      setDatabases((current) =>
        current.filter((item) => item.id !== database.id),
      );
      setTestResults((current) => {
        const next = { ...current };
        delete next[database.id];
        return next;
      });

      if (editingId === database.id) {
        closeForm();
      }

      toast.success(t.databasePicker.deleted);
    } catch (error) {
      toast.error(
        error instanceof Error ? error.message : t.databasePicker.deleteFailed,
      );
    } finally {
      setDeletingId(null);
    }
  };

  const handleTestConnection = async (database: ManagedDatabase) => {
    setTestingId(database.id);

    try {
      const response = await apiRequest<ManagedDatabaseConnectionTestResponse>(
        `/api/v1/managed-databases/${database.id}/test-connection`,
        {
          method: "POST",
          token,
          body: {},
        },
      );
      setTestResults((current) => ({
        ...current,
        [database.id]: {
          tone: "success",
          message: response.message,
        },
      }));
    } catch (error) {
      setTestResults((current) => ({
        ...current,
        [database.id]: {
          tone: "error",
          message:
            error instanceof Error ? error.message : t.databasePicker.testFailed,
        },
      }));
    } finally {
      setTestingId(null);
    }
  };

  const handleEnterWorkspace = async (database: ManagedDatabase) => {
    setEnteringId(database.id);

    try {
      const body: SetCurrentManagedDatabaseRequest = {
        managed_database_id: database.id,
      };
      const response = await apiRequest<CurrentManagedDatabaseResponse>(
        "/api/v1/managed-databases/current",
        {
          method: "PUT",
          token,
          body,
        },
      );
      onDatabaseSelected(response.database ?? database);
    } catch (error) {
      toast.error(
        error instanceof Error ? error.message : t.databasePicker.enterFailed,
      );
    } finally {
      setEnteringId(null);
    }
  };

  return (
    <main className="min-h-screen bg-background text-foreground">
      <div className="flex min-h-screen flex-col">
        <header className="sticky top-0 z-30 border-b bg-background">
          <nav className="flex h-16 items-center justify-between gap-3 px-3 sm:px-4 lg:px-5">
            <div className="flex min-w-0 items-center">
              <Image
                src="/banner.png"
                alt="Liquid"
                width={217}
                height={72}
                priority
                unoptimized
                draggable={false}
                className="h-10 w-auto max-w-[150px] select-none object-contain sm:h-11 sm:max-w-[210px]"
              />
              <h1 className="sr-only">{t.databasePicker.title}</h1>
            </div>

            <div className="flex min-w-0 items-center justify-end gap-2">
              <Button
                type="button"
                variant="outline"
                size="icon"
                className="size-9 rounded-lg"
                title={themeShortcutLabel}
                aria-label={themeShortcutLabel}
                onClick={handleThemeShortcut}
              >
                <ThemeShortcutIcon className="size-4" aria-hidden />
              </Button>
              <Button
                type="button"
                variant="outline"
                size="icon"
                className="size-9 rounded-lg"
                title={languageShortcutLabel}
                aria-label={languageShortcutLabel}
                onClick={handleLanguageShortcut}
              >
                <Languages className="size-4" aria-hidden />
              </Button>

              <div className="relative flex justify-end" ref={accountMenuRef}>
                <button
                  type="button"
                  className={cn(
                    "group flex h-9 min-w-0 items-center gap-2 rounded-lg border bg-background px-2 text-left shadow-xs outline-none transition-all hover:border-foreground/20 hover:bg-accent/70 focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 sm:gap-3 sm:px-2.5",
                    isAccountMenuOpen && "border-foreground/20 bg-accent/70",
                  )}
                  aria-haspopup="menu"
                  aria-expanded={isAccountMenuOpen}
                  aria-label={t.databasePicker.accountMenuLabel(user.display_name)}
                  onClick={() => setIsAccountMenuOpen((isOpen) => !isOpen)}
                >
                  <span className="flex size-7 shrink-0 items-center justify-center rounded-full bg-primary text-xs font-semibold text-primary-foreground shadow-sm sm:size-8">
                    {userInitials}
                  </span>
                  <span className="hidden max-w-32 truncate text-sm font-medium text-foreground sm:inline">
                    {user.display_name}
                  </span>
                  <ChevronDown
                    className={cn(
                      "size-4 text-muted-foreground transition-transform group-hover:text-foreground",
                      isAccountMenuOpen && "rotate-180",
                    )}
                    aria-hidden
                  />
                </button>
                {isAccountMenuOpen ? (
                  <div
                    className="absolute right-0 top-[calc(100%+0.5rem)] z-20 w-64 overflow-hidden rounded-xl border bg-popover p-2 text-popover-foreground shadow-lg"
                    role="menu"
                  >
                    <div className="flex items-center gap-3 rounded-lg bg-muted/60 p-2.5">
                      <span className="flex size-9 shrink-0 items-center justify-center rounded-full bg-primary text-xs font-semibold text-primary-foreground">
                        {userInitials}
                      </span>
                      <div className="min-w-0">
                        <div className="truncate text-sm font-semibold">
                          {user.display_name}
                        </div>
                        <div className="truncate text-xs text-muted-foreground">
                          {user.email}
                        </div>
                      </div>
                    </div>
                    <div className="my-1.5 h-px bg-border" />
                    <button
                      type="button"
                      className="flex w-full items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-sm outline-none transition-colors hover:bg-accent hover:text-accent-foreground focus-visible:bg-accent focus-visible:text-accent-foreground"
                      role="menuitem"
                      onClick={openSettings}
                    >
                      <Settings className="size-4" aria-hidden />
                      {t.databasePicker.settings}
                    </button>
                    <button
                      type="button"
                      className="flex w-full items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-sm text-destructive outline-none transition-colors hover:bg-destructive/10 focus-visible:bg-destructive/10"
                      role="menuitem"
                      onClick={handleLogout}
                    >
                      <LogOut className="size-4" aria-hidden />
                      {t.databasePicker.logout}
                    </button>
                  </div>
                ) : null}
              </div>
            </div>
          </nav>
        </header>

        <div className="flex min-h-0 flex-1 flex-col lg:flex-row">
          <aside className="border-b bg-muted/20 px-3 py-3 lg:w-64 lg:shrink-0 lg:border-b-0 lg:border-r lg:px-4 lg:py-5">
            <nav
              className="flex gap-2 overflow-x-auto lg:flex-col lg:overflow-visible"
              aria-label={t.databasePicker.navigationLabel}
            >
              {menuItems.map((item) => {
                const Icon = item.icon;
                const isActive = activeView === item.value;

                return (
                  <button
                    key={item.value}
                    type="button"
                    className={cn(
                      "group flex min-w-[15rem] items-center justify-between gap-3 rounded-lg border px-3 py-3 text-left outline-none transition-all focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 lg:min-w-0",
                      isActive
                        ? "border-border bg-background text-foreground shadow-xs"
                        : "border-transparent text-muted-foreground hover:border-border/70 hover:bg-background/70 hover:text-foreground",
                    )}
                    aria-current={isActive ? "page" : undefined}
                    onClick={() => setActiveView(item.value)}
                  >
                    <span className="flex min-w-0 items-center gap-3">
                      <span
                        className={cn(
                          "flex size-9 shrink-0 items-center justify-center rounded-md border bg-background shadow-xs transition-colors",
                          isActive
                            ? "border-primary bg-primary text-primary-foreground"
                            : "text-muted-foreground group-hover:text-foreground",
                        )}
                      >
                        <Icon className="size-4" aria-hidden />
                      </span>
                      <span className="min-w-0">
                        <span className="block truncate text-sm font-semibold">
                          {item.label}
                        </span>
                        <span className="mt-1 block truncate text-xs text-muted-foreground">
                          {item.description}
                        </span>
                      </span>
                    </span>
                    {item.badge ? (
                      <Badge variant="secondary" className="rounded-md">
                        {item.badge}
                      </Badge>
                    ) : null}
                  </button>
                );
              })}
            </nav>
          </aside>

          <section className="flex min-w-0 flex-1 flex-col bg-muted/30 p-3 sm:p-4 lg:p-5">
            {activeView === "overview" ? (
              <Card className="min-h-[420px] flex-1 rounded-lg py-4 shadow-xs">
              <CardHeader className="flex flex-row items-center justify-between gap-3 px-4">
                <div>
                  <CardTitle className="text-sm">
                    {t.databasePicker.workspaceTitle}
                  </CardTitle>
                  <p className="mt-1 text-xs text-muted-foreground">
                    {t.databasePicker.connectionCount(databases.length)}
                  </p>
                </div>
                <div className="flex flex-wrap items-center justify-end gap-2">
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={openCreateForm}
                  >
                    <Plus className="size-4" aria-hidden />
                    {t.databasePicker.addConnection}
                  </Button>
                </div>
              </CardHeader>
              <CardContent className="flex min-h-0 flex-1 flex-col px-4">
                <div className="relative">
                  <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
                  <input
                    value={query}
                    onChange={(event) => setQuery(event.target.value)}
                    placeholder={t.databasePicker.searchPlaceholder}
                    className="h-9 w-full rounded-md border bg-background pl-9 pr-3 text-sm outline-none transition-shadow placeholder:text-muted-foreground focus-visible:ring-[3px] focus-visible:ring-ring/50"
                  />
                </div>

                <div className="mt-3 min-h-0 flex-1 space-y-2 overflow-y-auto">
                  {isLoading ? (
                    <div className="flex items-center gap-2 rounded-lg border bg-background p-3 text-sm text-muted-foreground">
                      <Loader2 className="size-4 animate-spin" aria-hidden />
                      {t.databasePicker.loadingConnections}
                    </div>
                  ) : databases.length === 0 ? (
                    <EmptyDatabaseState />
                  ) : filteredDatabases.length === 0 ? (
                    <div className="rounded-lg border bg-background p-4 text-sm text-muted-foreground">
                      {t.databasePicker.noMatches}
                    </div>
                  ) : (
                    filteredDatabases.map((database) => (
                      <DatabaseListItem
                        key={database.id}
                        database={database}
                        testState={testResults[database.id]}
                        isTesting={testingId === database.id}
                        isEntering={enteringId === database.id}
                        isDeleting={deletingId === database.id}
                        onEdit={() => handleEdit(database)}
                        onTest={() => void handleTestConnection(database)}
                        onEnter={() => void handleEnterWorkspace(database)}
                        onDelete={() => void handleDelete(database)}
                      />
                    ))
                  )}
                </div>
              </CardContent>
              </Card>
            ) : null}
            {activeView === "audits" ? (
              <SqlAuditHistory token={token} databases={databases} />
            ) : null}
            {activeView === "backups" ? <BackupPlaceholder /> : null}
          </section>
        </div>
      </div>
      {isFormOpen ? (
        <ManagedDatabaseFormDialog
          editingId={editingId}
          form={form}
          error={formError}
          isSubmitting={isSubmitting}
          onClose={closeForm}
          onSubmit={handleSubmit}
          onFormChange={setForm}
        />
      ) : null}
      {isSettingsOpen ? (
        <AccountSettingsDialog
          token={token}
          user={user}
          onClose={() => setIsSettingsOpen(false)}
          onUserUpdated={onUserUpdated}
        />
      ) : null}
    </main>
  );
}

function DatabaseListItem({
  database,
  testState,
  isTesting,
  isEntering,
  isDeleting,
  onEdit,
  onTest,
  onEnter,
  onDelete,
}: {
  database: ManagedDatabase;
  testState?: ConnectionTestState;
  isTesting: boolean;
  isEntering: boolean;
  isDeleting: boolean;
  onEdit: () => void;
  onTest: () => void;
  onEnter: () => void;
  onDelete: () => void;
}) {
  const { t } = useI18n();
  const isBusy = isTesting || isEntering || isDeleting;

  return (
    <article className="rounded-lg border bg-background p-3 shadow-xs transition-colors hover:bg-muted/30">
      <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
        <div className="min-w-0">
          <div className="flex min-w-0 items-center gap-2">
            <Database className="size-4 shrink-0 text-muted-foreground" />
            <div className="truncate text-sm font-medium">{database.name}</div>
          </div>
          <div className="mt-1 truncate text-xs text-muted-foreground">
            {database.username}@{database.host}:{database.port} /{" "}
            {database.database}
          </div>
          <div className="mt-2 flex flex-wrap gap-1.5">
            {(database.tags ?? []).map((tag) => (
              <Badge
                key={tag}
                variant="secondary"
                className="rounded-md bg-primary/10 text-foreground"
              >
                {tag}
              </Badge>
            ))}
            <Badge variant="outline" className="rounded-md">
              {database.engine}
            </Badge>
            <Badge variant="outline" className="rounded-md">
              SSL {database.ssl_mode}
            </Badge>
            <Badge variant="secondary" className="rounded-md">
              {database.has_password
                ? t.databasePicker.passwordSaved
                : t.databasePicker.noPassword}
            </Badge>
            {testState ? (
              <Badge
                variant={testState.tone === "success" ? "secondary" : "destructive"}
                className="rounded-md"
              >
                {testState.tone === "success"
                  ? t.databasePicker.connectionAvailable
                  : t.databasePicker.connectionIssue}
              </Badge>
            ) : null}
          </div>
        </div>

        <div className="grid grid-cols-[repeat(4,2.25rem)] items-center justify-end gap-2 md:w-auto">
          <Button
            type="button"
            size="icon"
            className="size-9"
            title={t.databasePicker.enter}
            aria-label={t.databasePicker.enterDatabaseLabel(database.name)}
            disabled={isBusy}
            onClick={onEnter}
          >
            {isEntering ? (
              <Loader2 className="size-4 animate-spin" aria-hidden />
            ) : (
              <LogIn className="size-4" aria-hidden />
            )}
          </Button>
          <Button
            type="button"
            variant="outline"
            size="icon"
            className="size-9"
            title={t.databasePicker.testConnection}
            aria-label={t.databasePicker.testDatabaseLabel(database.name)}
            disabled={isBusy}
            onClick={onTest}
          >
            {isTesting ? (
              <Loader2 className="size-4 animate-spin" aria-hidden />
            ) : (
              <Plug className="size-4" aria-hidden />
            )}
          </Button>
          <Button
            type="button"
            variant="outline"
            size="icon"
            className="size-9"
            title={t.databasePicker.editConnection}
            aria-label={t.databasePicker.editDatabaseLabel(database.name)}
            disabled={isDeleting}
            onClick={onEdit}
          >
            <PencilLine className="size-4" aria-hidden />
          </Button>
          <Button
            type="button"
            variant="outline"
            size="icon"
            className="size-9 border-destructive/20 text-destructive hover:bg-destructive/10 hover:text-destructive"
            title={t.databasePicker.deleteConnection}
            aria-label={t.databasePicker.deleteDatabaseLabel(database.name)}
            disabled={isBusy}
            onClick={onDelete}
          >
            {isDeleting ? (
              <Loader2 className="size-4 animate-spin" aria-hidden />
            ) : (
              <Trash2 className="size-4" aria-hidden />
            )}
          </Button>
        </div>
      </div>
      {testState ? (
        <div
          className={cn(
            "mt-3 flex items-center gap-2 rounded-md border px-3 py-2 text-xs",
            testState.tone === "success"
              ? "border-emerald-200 bg-emerald-50 text-emerald-800"
              : "border-destructive/30 bg-destructive/10 text-destructive",
          )}
        >
          {testState.tone === "success" ? (
            <CircleCheck
              className="size-4 shrink-0 text-emerald-600"
              aria-hidden
            />
          ) : (
            <CircleAlert className="size-4 shrink-0" aria-hidden />
          )}
          <span className="min-w-0">{testState.message}</span>
        </div>
      ) : null}
    </article>
  );
}

function ManagedDatabaseFormDialog({
  editingId,
  form,
  error,
  isSubmitting,
  onClose,
  onSubmit,
  onFormChange,
}: {
  editingId: string | null;
  form: ManagedDatabaseForm;
  error: string | null;
  isSubmitting: boolean;
  onClose: () => void;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
  onFormChange: (
    updater: (current: ManagedDatabaseForm) => ManagedDatabaseForm,
  ) => void;
}) {
  const { t } = useI18n();

  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center overflow-y-auto bg-background/80 p-3 sm:items-center">
      <button
        type="button"
        className="absolute inset-0 cursor-default"
        aria-label={t.databasePicker.closeDialog}
        disabled={isSubmitting}
        onClick={onClose}
      />
      <Card
        className="relative w-full max-w-3xl overflow-hidden rounded-xl py-0 shadow-xl"
        role="dialog"
        aria-modal="true"
        aria-labelledby="managed-database-dialog-title"
      >
        <CardHeader className="flex flex-row items-start justify-between gap-4 border-b bg-muted/30 px-5 py-4">
          <div className="flex min-w-0 items-start gap-3">
            <span className="mt-0.5 flex size-10 shrink-0 items-center justify-center rounded-lg bg-primary text-primary-foreground shadow-sm">
              <Database className="size-5" aria-hidden />
            </span>
            <div className="min-w-0">
              <CardTitle
                id="managed-database-dialog-title"
                className="text-base"
              >
                {editingId
                  ? t.databasePicker.editDialogTitle
                  : t.databasePicker.createDialogTitle}
              </CardTitle>
              <p className="mt-1 text-xs text-muted-foreground">
                {t.databasePicker.encryptedDescription}
              </p>
            </div>
          </div>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            aria-label={t.common.close}
            title={t.common.close}
            disabled={isSubmitting}
            onClick={onClose}
          >
            <X className="size-4" aria-hidden />
          </Button>
        </CardHeader>
        <CardContent className="px-5 py-4">
          <form className="space-y-4" onSubmit={onSubmit}>
            <FormSection
              icon={<Server className="size-4" aria-hidden />}
              title={t.databasePicker.sections.connection}
            >
              <div className="grid gap-3 sm:grid-cols-12">
                <DatabaseField
                  className="sm:col-span-4"
                  id="managed-name"
                  label={t.databasePicker.fields.name}
                  value={form.name}
                  onChange={(name) =>
                    onFormChange((current) => ({ ...current, name }))
                  }
                  required
                />
                <DatabaseField
                  className="sm:col-span-6"
                  id="managed-host"
                  label={t.databasePicker.fields.host}
                  value={form.host}
                  onChange={(host) =>
                    onFormChange((current) => ({ ...current, host }))
                  }
                  required
                />
                <DatabaseField
                  className="sm:col-span-2"
                  id="managed-port"
                  label={t.databasePicker.fields.port}
                  value={form.port}
                  onChange={(port) =>
                    onFormChange((current) => ({ ...current, port }))
                  }
                  inputMode="numeric"
                  required
                />
                <DatabaseField
                  className="sm:col-span-7"
                  id="managed-database"
                  label={t.databasePicker.fields.database}
                  value={form.database}
                  onChange={(database) =>
                    onFormChange((current) => ({ ...current, database }))
                  }
                  required
                />
                <div className="space-y-1.5 sm:col-span-5">
                  <label
                    className="text-xs font-medium text-muted-foreground"
                    htmlFor="managed-ssl"
                  >
                    SSL
                  </label>
                  <select
                    id="managed-ssl"
                    value={form.ssl_mode}
                    onChange={(event) =>
                      onFormChange((current) => ({
                        ...current,
                        ssl_mode: event.target
                          .value as ManagedDatabase["ssl_mode"],
                      }))
                    }
                    className="h-10 w-full rounded-md border bg-background px-3 text-sm outline-none transition-shadow focus-visible:ring-[3px] focus-visible:ring-ring/50"
                  >
                    <option value="disable">disable</option>
                    <option value="prefer">prefer</option>
                    <option value="require">require</option>
                  </select>
                </div>
              </div>
            </FormSection>

            <FormSection
              icon={<KeyRound className="size-4" aria-hidden />}
              title={t.databasePicker.sections.auth}
            >
              <div className="grid gap-3 sm:grid-cols-12">
                <DatabaseField
                  className="sm:col-span-5"
                  id="managed-username"
                  label={t.databasePicker.fields.username}
                  value={form.username}
                  onChange={(username) =>
                    onFormChange((current) => ({ ...current, username }))
                  }
                  required
                />
                <DatabaseField
                  className="sm:col-span-7"
                  id="managed-password"
                  label={
                    editingId
                      ? t.databasePicker.fields.newPassword
                      : t.databasePicker.fields.password
                  }
                  type="password"
                  value={form.password}
                  onChange={(password) =>
                    onFormChange((current) => ({ ...current, password }))
                  }
                  placeholder={
                    editingId
                      ? t.databasePicker.fields.keepPasswordPlaceholder
                      : ""
                  }
                  required={!editingId}
                />
              </div>
            </FormSection>

            <FormSection
              icon={<Tags className="size-4" aria-hidden />}
              title={t.databasePicker.sections.tags}
            >
              <TagInput
                id="managed-tags"
                label={t.databasePicker.fields.tags}
                tags={form.tags}
                onChange={(tags) =>
                  onFormChange((current) => ({ ...current, tags }))
                }
                placeholder={t.databasePicker.fields.tagsPlaceholder}
                duplicateMessage={t.databasePicker.tagDuplicate}
                emptyMessage={t.databasePicker.tagEmpty}
                removeLabel={t.databasePicker.removeTagLabel}
              />
            </FormSection>

          {error ? (
            <div className="mt-3 rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
              {error}
            </div>
          ) : null}
            <div className="-mx-5 -mb-4 mt-4 flex flex-col-reverse gap-2 border-t bg-muted/20 px-5 py-4 sm:flex-row sm:justify-end">
              <Button
                type="button"
                variant="outline"
                disabled={isSubmitting}
                onClick={onClose}
              >
                {t.common.cancel}
              </Button>
              <Button type="submit" disabled={isSubmitting}>
                {isSubmitting ? (
                  <Loader2 className="size-4 animate-spin" aria-hidden />
                ) : editingId ? (
                  <Save className="size-4" aria-hidden />
                ) : (
                  <Plus className="size-4" aria-hidden />
                )}
                {editingId
                  ? t.databasePicker.saveChanges
                  : t.databasePicker.saveConnection}
              </Button>
            </div>
          </form>
        </CardContent>
      </Card>
    </div>
  );
}

function FormSection({
  icon,
  title,
  children,
}: {
  icon: ReactNode;
  title: string;
  children: ReactNode;
}) {
  return (
    <section className="space-y-3 rounded-lg bg-muted/40 p-3">
      <div className="flex items-center gap-2 text-sm font-medium">
        <span className="flex size-7 items-center justify-center rounded-md bg-background text-muted-foreground shadow-xs">
          {icon}
        </span>
        {title}
      </div>
      {children}
    </section>
  );
}

function TagInput({
  id,
  label,
  tags,
  onChange,
  placeholder,
  duplicateMessage,
  emptyMessage,
  removeLabel,
}: {
  id: string;
  label: string;
  tags: string[];
  onChange: (tags: string[]) => void;
  placeholder: string;
  duplicateMessage: string;
  emptyMessage: string;
  removeLabel: (tag: string) => string;
}) {
  const [draft, setDraft] = useState("");
  const [feedback, setFeedback] = useState<string | null>(null);

  const addTags = (value: string) => {
    const candidates = value
      .split(",")
      .map((tag) => tag.trim())
      .filter(Boolean);

    if (candidates.length === 0) {
      setFeedback(emptyMessage);
      setDraft("");
      return;
    }

    const seen = new Set(tags.map((tag) => tag.toLowerCase()));
    const next = [...tags];

    for (const tag of candidates) {
      const key = tag.toLowerCase();

      if (seen.has(key)) {
        continue;
      }

      seen.add(key);
      next.push(tag);
    }

    if (next.length === tags.length) {
      setFeedback(duplicateMessage);
      setDraft("");
      return;
    }

    onChange(next);
    setDraft("");
    setFeedback(null);
  };

  const handleKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>) => {
    if (event.nativeEvent.isComposing) {
      return;
    }

    if (event.key === "Enter" || event.key === ",") {
      event.preventDefault();
      addTags(draft);
      return;
    }

    if (event.key === "Backspace" && !draft && tags.length > 0) {
      event.preventDefault();
      onChange(tags.slice(0, -1));
      setFeedback(null);
    }
  };

  const removeTag = (tag: string) => {
    onChange(tags.filter((current) => current !== tag));
    setFeedback(null);
  };

  return (
    <div className="space-y-1.5">
      <label className="text-xs font-medium text-muted-foreground" htmlFor={id}>
        {label}
      </label>
      <div
        className={cn(
          "flex min-h-10 w-full flex-wrap items-center gap-1.5 rounded-md border bg-background px-2 py-1.5 text-sm transition-shadow focus-within:ring-[3px] focus-within:ring-ring/50",
          feedback && "border-destructive/50 focus-within:ring-destructive/20",
        )}
      >
        {tags.map((tag) => (
          <span
            key={tag}
            className="inline-flex h-7 max-w-full items-center gap-1 rounded-md bg-primary/10 px-2 text-xs font-medium text-foreground"
          >
            <span className="truncate">{tag}</span>
            <button
              type="button"
              className="rounded-sm text-muted-foreground outline-none transition-colors hover:text-foreground focus-visible:ring-[2px] focus-visible:ring-ring/50"
              aria-label={removeLabel(tag)}
              title={removeLabel(tag)}
              onClick={() => removeTag(tag)}
            >
              <X className="size-3" aria-hidden />
            </button>
          </span>
        ))}
        <input
          id={id}
          value={draft}
          onChange={(event) => {
            setDraft(event.target.value);
            setFeedback(null);
          }}
          onBlur={() => {
            if (draft.trim()) {
              addTags(draft);
            }
          }}
          onKeyDown={handleKeyDown}
          placeholder={tags.length === 0 ? placeholder : ""}
          className="h-7 min-w-28 flex-1 bg-transparent px-1 text-sm outline-none placeholder:text-muted-foreground"
        />
      </div>
      {feedback ? (
        <p className="text-xs text-destructive">{feedback}</p>
      ) : null}
    </div>
  );
}

function EmptyDatabaseState() {
  const { t } = useI18n();

  return (
    <div className="rounded-lg border bg-background p-4 text-sm">
      <div className="flex items-center gap-2 font-medium">
        <Database className="size-4 text-muted-foreground" aria-hidden />
        {t.databasePicker.emptyTitle}
      </div>
      <p className="mt-2 text-xs leading-5 text-muted-foreground">
        {t.databasePicker.emptyDescription}
      </p>
    </div>
  );
}

function BackupPlaceholder() {
  const { t } = useI18n();

  return (
    <Card className="min-h-[420px] flex-1 rounded-lg py-4 shadow-xs">
      <CardContent className="flex min-h-[360px] flex-col items-center justify-center px-4 text-center">
        <div className="flex size-12 items-center justify-center rounded-lg border bg-muted/40">
          <Archive className="size-6 text-muted-foreground" aria-hidden />
        </div>
        <h2 className="mt-4 text-base font-semibold">
          {t.databasePicker.backupPlaceholderTitle}
        </h2>
        <p className="mt-2 max-w-md text-sm leading-6 text-muted-foreground">
          {t.databasePicker.backupPlaceholderDescription}
        </p>
        <Badge variant="secondary" className="mt-4 rounded-md">
          {t.databasePicker.backupPlaceholderBadge}
        </Badge>
      </CardContent>
    </Card>
  );
}

function DatabaseField({
  id,
  label,
  value,
  onChange,
  type = "text",
  placeholder,
  inputMode,
  required,
  className,
}: {
  id: string;
  label: string;
  value: string;
  onChange: (value: string) => void;
  type?: string;
  placeholder?: string;
  inputMode?: "numeric";
  required?: boolean;
  className?: string;
}) {
  return (
    <div className={cn("space-y-1.5", className)}>
      <label className="text-xs font-medium text-muted-foreground" htmlFor={id}>
        {label}
      </label>
      <input
        id={id}
        type={type}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder={placeholder}
        inputMode={inputMode}
        required={required}
        className="h-10 w-full rounded-md border bg-background px-3 text-sm outline-none transition-shadow placeholder:text-muted-foreground focus-visible:ring-[3px] focus-visible:ring-ring/50"
      />
    </div>
  );
}
