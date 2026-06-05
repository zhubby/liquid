"use client";

import { type FormEvent, useCallback, useEffect, useMemo, useState } from "react";
import {
  CheckCircle2,
  Database,
  Loader2,
  LogOut,
  Plus,
  Save,
  Search,
  ShieldCheck,
  Trash2,
  X,
} from "lucide-react";

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
import { cn } from "@/lib/utils";

type ManagedDatabasePickerProps = {
  token: string;
  user: PublicUser;
  onDatabaseSelected: (database: ManagedDatabase) => void;
  onLogout: () => void;
};

type ManagedDatabaseForm = {
  name: string;
  host: string;
  port: string;
  database: string;
  username: string;
  password: string;
  ssl_mode: ManagedDatabase["ssl_mode"];
};

type ConnectionTestState = {
  tone: "success" | "error";
  message: string;
};

const emptyManagedDatabaseForm: ManagedDatabaseForm = {
  name: "",
  host: "",
  port: "5432",
  database: "",
  username: "",
  password: "",
  ssl_mode: "prefer",
};

export function ManagedDatabasePicker({
  token,
  user,
  onDatabaseSelected,
  onLogout,
}: ManagedDatabasePickerProps) {
  const [databases, setDatabases] = useState<ManagedDatabase[]>([]);
  const [selectedId, setSelectedId] = useState("");
  const [query, setQuery] = useState("");
  const [form, setForm] = useState<ManagedDatabaseForm>(
    emptyManagedDatabaseForm,
  );
  const [editingId, setEditingId] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [testingId, setTestingId] = useState<string | null>(null);
  const [enteringId, setEnteringId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [testResults, setTestResults] = useState<
    Record<string, ConnectionTestState>
  >({});

  const loadDatabases = useCallback(async () => {
    setIsLoading(true);
    setError(null);

    try {
      const response = await apiRequest<ManagedDatabase[]>(
        "/api/v1/managed-databases",
        { token },
      );
      setDatabases(response);
      setSelectedId((current) =>
        current && response.some((database) => database.id === current)
          ? current
          : response[0]?.id ?? "",
      );
    } catch (error) {
      setError(error instanceof Error ? error.message : "加载托管数据库失败");
    } finally {
      setIsLoading(false);
    }
  }, [token]);

  useEffect(() => {
    void loadDatabases();
  }, [loadDatabases]);

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
      ]
        .join(" ")
        .toLowerCase()
        .includes(term),
    );
  }, [databases, query]);

  const selectedDatabase = databases.find(
    (database) => database.id === selectedId,
  );

  const resetForm = () => {
    setForm(emptyManagedDatabaseForm);
    setEditingId(null);
  };

  const handleSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setError(null);
    setStatus(null);
    setIsSubmitting(true);

    const port = Number.parseInt(form.port, 10);

    if (!Number.isInteger(port) || port < 1 || port > 65_535) {
      setError("端口必须是 1-65535 的数字");
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
        setSelectedId(updated.id);
        setStatus("连接记录已更新");
      } else {
        const body: CreateManagedDatabaseRequest = {
          name: form.name,
          engine: "postgres",
          host: form.host,
          port,
          database: form.database,
          username: form.username,
          password: form.password,
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
        setSelectedId(created.id);
        setStatus("连接记录已创建");
      }

      resetForm();
    } catch (error) {
      setError(error instanceof Error ? error.message : "保存数据库失败");
    } finally {
      setIsSubmitting(false);
    }
  };

  const handleEdit = (database: ManagedDatabase) => {
    setEditingId(database.id);
    setSelectedId(database.id);
    setForm({
      name: database.name,
      host: database.host,
      port: String(database.port),
      database: database.database,
      username: database.username,
      password: "",
      ssl_mode: database.ssl_mode,
    });
    setStatus(null);
    setError(null);
  };

  const handleDelete = async (database: ManagedDatabase) => {
    setError(null);
    setStatus(null);

    try {
      await apiRequest<void>(`/api/v1/managed-databases/${database.id}`, {
        method: "DELETE",
        token,
      });
      setDatabases((current) =>
        current.filter((item) => item.id !== database.id),
      );
      setSelectedId((current) => (current === database.id ? "" : current));
      setTestResults((current) => {
        const next = { ...current };
        delete next[database.id];
        return next;
      });

      if (editingId === database.id) {
        resetForm();
      }

      setStatus("连接记录已删除");
    } catch (error) {
      setError(error instanceof Error ? error.message : "删除数据库失败");
    }
  };

  const handleTestConnection = async (database: ManagedDatabase) => {
    setTestingId(database.id);
    setError(null);

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
          message: error instanceof Error ? error.message : "连接测试失败",
        },
      }));
    } finally {
      setTestingId(null);
    }
  };

  const handleEnterWorkspace = async (database: ManagedDatabase) => {
    setEnteringId(database.id);
    setError(null);

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
      setError(error instanceof Error ? error.message : "进入工作区失败");
    } finally {
      setEnteringId(null);
    }
  };

  return (
    <main className="min-h-screen bg-muted/30 p-3 text-foreground">
      <div className="mx-auto flex min-h-[calc(100vh-1.5rem)] w-full max-w-7xl flex-col gap-3">
        <Card className="rounded-lg py-4 shadow-xs">
          <CardContent className="flex flex-col gap-3 px-4 sm:flex-row sm:items-center sm:justify-between">
            <div className="flex min-w-0 items-center gap-3">
              <div className="flex size-10 shrink-0 items-center justify-center rounded-md bg-primary text-primary-foreground">
                <ShieldCheck className="size-5" aria-hidden />
              </div>
              <div className="min-w-0">
                <h1 className="truncate text-base font-semibold">
                  选择托管数据库
                </h1>
                <p className="mt-1 truncate text-xs text-muted-foreground">
                  Liquid SQL Audit / {user.display_name}
                </p>
              </div>
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <Badge variant="secondary" className="rounded-md">
                {user.email}
              </Badge>
              <Button type="button" variant="outline" size="sm" onClick={onLogout}>
                <LogOut className="size-4" aria-hidden />
                注销
              </Button>
            </div>
          </CardContent>
        </Card>

        <div className="grid min-h-0 flex-1 gap-3 lg:grid-cols-[minmax(320px,0.9fr)_minmax(460px,1.1fr)]">
          <Card className="min-h-[420px] rounded-lg py-4 shadow-xs">
            <CardHeader className="flex flex-row items-center justify-between gap-3 px-4">
              <div>
                <CardTitle className="text-sm">数据库工作区</CardTitle>
                <p className="mt-1 text-xs text-muted-foreground">
                  {databases.length} 个连接记录
                </p>
              </div>
              <Badge variant="outline" className="rounded-md">
                Postgres
              </Badge>
            </CardHeader>
            <CardContent className="flex min-h-0 flex-1 flex-col px-4">
              <div className="relative">
                <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
                <input
                  value={query}
                  onChange={(event) => setQuery(event.target.value)}
                  placeholder="搜索名称、主机、数据库"
                  className="h-9 w-full rounded-md border bg-background pl-9 pr-3 text-sm outline-none transition-shadow placeholder:text-muted-foreground focus-visible:ring-[3px] focus-visible:ring-ring/50"
                />
              </div>

              <div className="mt-3 min-h-0 flex-1 space-y-2 overflow-y-auto">
                {isLoading ? (
                  <div className="flex items-center gap-2 rounded-lg border bg-background p-3 text-sm text-muted-foreground">
                    <Loader2 className="size-4 animate-spin" aria-hidden />
                    正在加载连接记录
                  </div>
                ) : databases.length === 0 ? (
                  <EmptyDatabaseState />
                ) : filteredDatabases.length === 0 ? (
                  <div className="rounded-lg border bg-background p-4 text-sm text-muted-foreground">
                    没有匹配的数据库
                  </div>
                ) : (
                  filteredDatabases.map((database) => (
                    <DatabaseListItem
                      key={database.id}
                      database={database}
                      active={database.id === selectedId}
                      testState={testResults[database.id]}
                      isTesting={testingId === database.id}
                      isEntering={enteringId === database.id}
                      onSelect={() => setSelectedId(database.id)}
                      onEdit={() => handleEdit(database)}
                      onDelete={() => void handleDelete(database)}
                      onTest={() => void handleTestConnection(database)}
                      onEnter={() => void handleEnterWorkspace(database)}
                    />
                  ))
                )}
              </div>
            </CardContent>
          </Card>

          <Card className="rounded-lg py-4 shadow-xs">
            <CardHeader className="flex flex-row items-center justify-between gap-3 px-4">
              <div>
                <CardTitle className="text-sm">
                  {editingId ? "编辑连接" : "新增连接"}
                </CardTitle>
                <p className="mt-1 text-xs text-muted-foreground">
                  连接信息加密保存
                </p>
              </div>
              <Database className="size-4 shrink-0 text-muted-foreground" />
            </CardHeader>
            <CardContent className="px-4">
              <form className="grid gap-3 lg:grid-cols-12" onSubmit={handleSubmit}>
                <DatabaseField
                  className="lg:col-span-4"
                  id="managed-name"
                  label="名称"
                  value={form.name}
                  onChange={(name) =>
                    setForm((current) => ({ ...current, name }))
                  }
                  required
                />
                <DatabaseField
                  className="lg:col-span-5"
                  id="managed-host"
                  label="主机"
                  value={form.host}
                  onChange={(host) =>
                    setForm((current) => ({ ...current, host }))
                  }
                  required
                />
                <DatabaseField
                  className="lg:col-span-3"
                  id="managed-port"
                  label="端口"
                  value={form.port}
                  onChange={(port) =>
                    setForm((current) => ({ ...current, port }))
                  }
                  inputMode="numeric"
                  required
                />
                <DatabaseField
                  className="lg:col-span-5"
                  id="managed-database"
                  label="数据库"
                  value={form.database}
                  onChange={(database) =>
                    setForm((current) => ({ ...current, database }))
                  }
                  required
                />
                <DatabaseField
                  className="lg:col-span-4"
                  id="managed-username"
                  label="用户名"
                  value={form.username}
                  onChange={(username) =>
                    setForm((current) => ({ ...current, username }))
                  }
                  required
                />
                <div className="space-y-1.5 lg:col-span-3">
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
                      setForm((current) => ({
                        ...current,
                        ssl_mode: event.target.value as ManagedDatabase["ssl_mode"],
                      }))
                    }
                    className="h-9 w-full rounded-md border bg-background px-3 text-sm outline-none transition-shadow focus-visible:ring-[3px] focus-visible:ring-ring/50"
                  >
                    <option value="disable">disable</option>
                    <option value="prefer">prefer</option>
                    <option value="require">require</option>
                  </select>
                </div>
                <DatabaseField
                  className="lg:col-span-6"
                  id="managed-password"
                  label={editingId ? "新密码" : "密码"}
                  type="password"
                  value={form.password}
                  onChange={(password) =>
                    setForm((current) => ({ ...current, password }))
                  }
                  placeholder={editingId ? "留空则保持原密码" : ""}
                  required={!editingId}
                />
                <div className="flex items-end gap-2 lg:col-span-6">
                  <Button type="submit" disabled={isSubmitting}>
                    {isSubmitting ? (
                      <Loader2 className="size-4 animate-spin" aria-hidden />
                    ) : editingId ? (
                      <Save className="size-4" aria-hidden />
                    ) : (
                      <Plus className="size-4" aria-hidden />
                    )}
                    {editingId ? "保存修改" : "保存连接"}
                  </Button>
                  {editingId ? (
                    <Button type="button" variant="outline" onClick={resetForm}>
                      <X className="size-4" aria-hidden />
                      取消
                    </Button>
                  ) : null}
                </div>
              </form>

              {error ? (
                <div className="mt-3 rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
                  {error}
                </div>
              ) : null}
              {status ? (
                <div className="mt-3 rounded-md border bg-secondary px-3 py-2 text-sm text-secondary-foreground">
                  {status}
                </div>
              ) : null}

              <div className="mt-4 rounded-lg border bg-background p-3">
                <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
                  <div className="min-w-0">
                    <div className="text-xs font-medium text-muted-foreground">
                      当前选择
                    </div>
                    <div className="mt-1 truncate text-sm font-medium">
                      {selectedDatabase
                        ? `${selectedDatabase.name} / ${selectedDatabase.database}`
                        : "未选择数据库"}
                    </div>
                  </div>
                  <div className="flex flex-wrap gap-2">
                    <Button
                      type="button"
                      variant="outline"
                      disabled={!selectedDatabase || testingId !== null}
                      onClick={() =>
                        selectedDatabase
                          ? void handleTestConnection(selectedDatabase)
                          : undefined
                      }
                    >
                      {selectedDatabase && testingId === selectedDatabase.id ? (
                        <Loader2 className="size-4 animate-spin" aria-hidden />
                      ) : (
                        <ShieldCheck className="size-4" aria-hidden />
                      )}
                      测试连接
                    </Button>
                    <Button
                      type="button"
                      disabled={!selectedDatabase || enteringId !== null}
                      onClick={() =>
                        selectedDatabase
                          ? void handleEnterWorkspace(selectedDatabase)
                          : undefined
                      }
                    >
                      {selectedDatabase && enteringId === selectedDatabase.id ? (
                        <Loader2 className="size-4 animate-spin" aria-hidden />
                      ) : (
                        <CheckCircle2 className="size-4" aria-hidden />
                      )}
                      进入工作区
                    </Button>
                  </div>
                </div>
                {selectedDatabase && testResults[selectedDatabase.id] ? (
                  <div
                    className={cn(
                      "mt-3 rounded-md border px-3 py-2 text-sm",
                      testResults[selectedDatabase.id].tone === "success"
                        ? "bg-secondary text-secondary-foreground"
                        : "border-destructive/30 bg-destructive/10 text-destructive",
                    )}
                  >
                    {testResults[selectedDatabase.id].message}
                  </div>
                ) : null}
              </div>
            </CardContent>
          </Card>
        </div>
      </div>
    </main>
  );
}

function DatabaseListItem({
  database,
  active,
  testState,
  isTesting,
  isEntering,
  onSelect,
  onEdit,
  onDelete,
  onTest,
  onEnter,
}: {
  database: ManagedDatabase;
  active: boolean;
  testState?: ConnectionTestState;
  isTesting: boolean;
  isEntering: boolean;
  onSelect: () => void;
  onEdit: () => void;
  onDelete: () => void;
  onTest: () => void;
  onEnter: () => void;
}) {
  return (
    <article
      className={cn(
        "rounded-lg border bg-background p-3 shadow-xs transition-colors",
        active && "border-primary/35 bg-secondary/60 ring-1 ring-ring/20",
      )}
    >
      <div className="flex items-start gap-3">
        <button
          type="button"
          className="min-w-0 flex-1 text-left outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50"
          onClick={onSelect}
        >
          <div className="flex min-w-0 items-center gap-2">
            <Database className="size-4 shrink-0 text-muted-foreground" />
            <div className="truncate text-sm font-medium">{database.name}</div>
          </div>
          <div className="mt-1 truncate text-xs text-muted-foreground">
            {database.host}:{database.port} / {database.database}
          </div>
          <div className="mt-2 flex flex-wrap gap-1.5">
            <Badge variant="outline" className="rounded-md">
              {database.username}
            </Badge>
            <Badge variant="outline" className="rounded-md">
              SSL {database.ssl_mode}
            </Badge>
            <Badge variant="secondary" className="rounded-md">
              {database.has_password ? "密码已保存" : "无密码"}
            </Badge>
            {testState ? (
              <Badge
                variant={testState.tone === "success" ? "secondary" : "destructive"}
                className="rounded-md"
              >
                {testState.tone === "success" ? "连接可用" : "连接异常"}
              </Badge>
            ) : null}
          </div>
        </button>
      </div>
      <div className="mt-3 flex flex-wrap justify-end gap-2">
        <Button type="button" variant="outline" size="sm" onClick={onEdit}>
          编辑
        </Button>
        <Button
          type="button"
          variant="outline"
          size="sm"
          disabled={isTesting}
          onClick={onTest}
        >
          {isTesting ? (
            <Loader2 className="size-4 animate-spin" aria-hidden />
          ) : (
            <ShieldCheck className="size-4" aria-hidden />
          )}
          测试
        </Button>
        <Button
          type="button"
          size="sm"
          disabled={isEntering}
          onClick={onEnter}
        >
          {isEntering ? (
            <Loader2 className="size-4 animate-spin" aria-hidden />
          ) : (
            <CheckCircle2 className="size-4" aria-hidden />
          )}
          进入
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="size-8"
          aria-label={`删除 ${database.name}`}
          title="删除"
          onClick={onDelete}
        >
          <Trash2 className="size-4" aria-hidden />
        </Button>
      </div>
      {testState ? (
        <div
          className={cn(
            "mt-3 rounded-md border px-3 py-2 text-xs",
            testState.tone === "success"
              ? "bg-secondary text-secondary-foreground"
              : "border-destructive/30 bg-destructive/10 text-destructive",
          )}
        >
          {testState.message}
        </div>
      ) : null}
    </article>
  );
}

function EmptyDatabaseState() {
  return (
    <div className="rounded-lg border bg-background p-4 text-sm">
      <div className="flex items-center gap-2 font-medium">
        <Database className="size-4 text-muted-foreground" aria-hidden />
        暂无托管数据库
      </div>
      <p className="mt-2 text-xs leading-5 text-muted-foreground">
        创建连接记录后才能进入 SQL 风险工作区。
      </p>
    </div>
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
        className="h-9 w-full rounded-md border bg-background px-3 text-sm outline-none transition-shadow placeholder:text-muted-foreground focus-visible:ring-[3px] focus-visible:ring-ring/50"
      />
    </div>
  );
}
