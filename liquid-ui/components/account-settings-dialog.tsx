"use client";

import {
  type FormEvent,
  type ReactNode,
  useEffect,
  useMemo,
  useState,
} from "react";
import { useTheme } from "next-themes";
import {
  CheckCircle2,
  KeyRound,
  Loader2,
  Monitor,
  Moon,
  Save,
  Settings,
  Sun,
  User,
  X,
} from "lucide-react";
import { toast } from "sonner";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  type CurrentUserResponse,
  type LlmProviderApiMode,
  type LlmProviderSettingsResponse,
  type PublicUser,
  type UpdateCurrentUserRequest,
  type UpdateLlmProviderSettingsRequest,
  type UpdatePasswordRequest,
  apiRequest,
} from "@/lib/api";
import { cn } from "@/lib/utils";

type SettingsTab = "account" | "preferences" | "model";
type LanguagePreference = "zh-CN" | "en-US";
type ThemePreference = "system" | "light" | "dark";

type AccountSettingsDialogProps = {
  token: string;
  user: PublicUser;
  onClose: () => void;
  onUserUpdated: (user: PublicUser) => void;
};

const LANGUAGE_STORAGE_KEY = "liquid.preferences.language";

const tabs: Array<{ value: SettingsTab; label: string }> = [
  { value: "account", label: "账户" },
  { value: "preferences", label: "偏好" },
  { value: "model", label: "模型" },
];

export function AccountSettingsDialog({
  token,
  user,
  onClose,
  onUserUpdated,
}: AccountSettingsDialogProps) {
  const [activeTab, setActiveTab] = useState<SettingsTab>("account");
  const [displayName, setDisplayName] = useState(user.display_name);
  const [currentPassword, setCurrentPassword] = useState("");
  const [newPassword, setNewPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [language, setLanguage] = useState<LanguagePreference>("zh-CN");
  const [baseUrl, setBaseUrl] = useState("https://api.openai.com");
  const [model, setModel] = useState("");
  const [apiMode, setApiMode] =
    useState<LlmProviderApiMode>("chat_completions");
  const [apiKey, setApiKey] = useState("");
  const [hasApiKey, setHasApiKey] = useState(false);
  const [isLoadingModel, setIsLoadingModel] = useState(true);
  const [savingDisplayName, setSavingDisplayName] = useState(false);
  const [savingPassword, setSavingPassword] = useState(false);
  const [savingModel, setSavingModel] = useState(false);
  const { theme, setTheme } = useTheme();

  useEffect(() => {
    setDisplayName(user.display_name);
  }, [user.display_name]);

  useEffect(() => {
    const stored = window.localStorage.getItem(LANGUAGE_STORAGE_KEY);
    if (stored === "zh-CN" || stored === "en-US") {
      setLanguage(stored);
    }
  }, []);

  useEffect(() => {
    let isMounted = true;

    apiRequest<LlmProviderSettingsResponse>("/api/v1/settings/llm-provider", {
      token,
    })
      .then((response) => {
        if (!isMounted || !response.settings) {
          return;
        }
        setBaseUrl(response.settings.base_url);
        setModel(response.settings.model);
        setApiMode(response.settings.api_mode);
        setHasApiKey(response.settings.has_api_key);
      })
      .catch((error) => {
        toast.error(error instanceof Error ? error.message : "加载模型配置失败");
      })
      .finally(() => {
        if (isMounted) {
          setIsLoadingModel(false);
        }
      });

    return () => {
      isMounted = false;
    };
  }, [token]);

  const selectedTheme = useMemo<ThemePreference>(() => {
    if (theme === "light" || theme === "dark" || theme === "system") {
      return theme;
    }

    return "system";
  }, [theme]);

  const handleDisplayNameSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setSavingDisplayName(true);

    try {
      const body: UpdateCurrentUserRequest = {
        display_name: displayName,
      };
      const response = await apiRequest<CurrentUserResponse>("/api/v1/auth/me", {
        method: "PATCH",
        token,
        body,
      });
      onUserUpdated(response.user);
      toast.success("显示名称已更新");
    } catch (error) {
      toast.error(error instanceof Error ? error.message : "更新显示名称失败");
    } finally {
      setSavingDisplayName(false);
    }
  };

  const handlePasswordSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();

    if (newPassword !== confirmPassword) {
      toast.error("两次输入的新密码不一致");
      return;
    }

    setSavingPassword(true);

    try {
      const body: UpdatePasswordRequest = {
        current_password: currentPassword,
        new_password: newPassword,
      };
      await apiRequest<void>("/api/v1/auth/password", {
        method: "PATCH",
        token,
        body,
      });
      setCurrentPassword("");
      setNewPassword("");
      setConfirmPassword("");
      toast.success("密码已更新");
    } catch (error) {
      toast.error(error instanceof Error ? error.message : "更新密码失败");
    } finally {
      setSavingPassword(false);
    }
  };

  const handleLanguageChange = (nextLanguage: LanguagePreference) => {
    setLanguage(nextLanguage);
    window.localStorage.setItem(LANGUAGE_STORAGE_KEY, nextLanguage);
    toast.success("语言偏好已保存");
  };

  const handleModelSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setSavingModel(true);

    try {
      const body: UpdateLlmProviderSettingsRequest = {
        provider: "openai_compatible",
        base_url: baseUrl,
        model,
        api_mode: apiMode,
      };

      if (apiKey.trim()) {
        body.api_key = apiKey;
      }

      const response = await apiRequest<LlmProviderSettingsResponse>(
        "/api/v1/settings/llm-provider",
        {
          method: "PUT",
          token,
          body,
        },
      );
      setHasApiKey(Boolean(response.settings?.has_api_key));
      setApiKey("");
      toast.success("模型配置已保存");
    } catch (error) {
      toast.error(error instanceof Error ? error.message : "保存模型配置失败");
    } finally {
      setSavingModel(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center overflow-y-auto bg-background/80 p-3 sm:items-center">
      <button
        type="button"
        className="absolute inset-0 cursor-default"
        aria-label="关闭设置弹窗"
        onClick={onClose}
      />
      <Card
        className="relative w-full max-w-3xl overflow-hidden rounded-xl py-0 shadow-xl"
        role="dialog"
        aria-modal="true"
        aria-labelledby="account-settings-title"
      >
        <CardHeader className="flex flex-row items-center justify-between gap-3 border-b px-4 py-4 sm:px-5">
          <div className="min-w-0">
            <CardTitle
              id="account-settings-title"
              className="flex items-center gap-2 text-base"
            >
              <Settings className="size-4 text-muted-foreground" aria-hidden />
              设置
            </CardTitle>
            <p className="mt-1 truncate text-xs text-muted-foreground">
              {user.email}
            </p>
          </div>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            aria-label="关闭"
            title="关闭"
            onClick={onClose}
          >
            <X className="size-4" aria-hidden />
          </Button>
        </CardHeader>
        <CardContent className="grid gap-0 p-0 sm:grid-cols-[180px_1fr]">
          <div
            className="flex gap-2 overflow-x-auto border-b bg-muted/30 p-3 sm:flex-col sm:border-b-0 sm:border-r"
            role="tablist"
            aria-label="设置分类"
          >
            {tabs.map((tab) => (
              <button
                key={tab.value}
                type="button"
                id={`${tab.value}-settings-tab`}
                role="tab"
                aria-selected={activeTab === tab.value}
                aria-controls={`${tab.value}-settings-panel`}
                className={cn(
                  "h-9 shrink-0 rounded-md px-3 text-left text-sm font-medium outline-none transition-colors focus-visible:ring-[3px] focus-visible:ring-ring/50",
                  activeTab === tab.value
                    ? "bg-background text-foreground shadow-xs"
                    : "text-muted-foreground hover:bg-background/70 hover:text-foreground",
                )}
                onClick={() => setActiveTab(tab.value)}
              >
                {tab.label}
              </button>
            ))}
          </div>
          <div className="min-h-[420px] p-4 sm:p-5">
            {activeTab === "account" ? (
              <SettingsPanel id="account-settings-panel" labelledBy="account-settings-tab">
                <AccountTab
                  displayName={displayName}
                  setDisplayName={setDisplayName}
                  savingDisplayName={savingDisplayName}
                  currentPassword={currentPassword}
                  setCurrentPassword={setCurrentPassword}
                  newPassword={newPassword}
                  setNewPassword={setNewPassword}
                  confirmPassword={confirmPassword}
                  setConfirmPassword={setConfirmPassword}
                  savingPassword={savingPassword}
                  onDisplayNameSubmit={handleDisplayNameSubmit}
                  onPasswordSubmit={handlePasswordSubmit}
                />
              </SettingsPanel>
            ) : null}
            {activeTab === "preferences" ? (
              <SettingsPanel
                id="preferences-settings-panel"
                labelledBy="preferences-settings-tab"
              >
                <PreferencesTab
                  language={language}
                  onLanguageChange={handleLanguageChange}
                  theme={selectedTheme}
                  onThemeChange={(nextTheme) => setTheme(nextTheme)}
                />
              </SettingsPanel>
            ) : null}
            {activeTab === "model" ? (
              <SettingsPanel id="model-settings-panel" labelledBy="model-settings-tab">
                <ModelTab
                  baseUrl={baseUrl}
                  setBaseUrl={setBaseUrl}
                  model={model}
                  setModel={setModel}
                  apiMode={apiMode}
                  setApiMode={setApiMode}
                  apiKey={apiKey}
                  setApiKey={setApiKey}
                  hasApiKey={hasApiKey}
                  isLoading={isLoadingModel}
                  isSaving={savingModel}
                  onSubmit={handleModelSubmit}
                />
              </SettingsPanel>
            ) : null}
          </div>
        </CardContent>
      </Card>
    </div>
  );
}

function SettingsPanel({
  id,
  labelledBy,
  children,
}: {
  id: string;
  labelledBy: string;
  children: ReactNode;
}) {
  return (
    <div id={id} role="tabpanel" aria-labelledby={labelledBy}>
      {children}
    </div>
  );
}

function AccountTab({
  displayName,
  setDisplayName,
  savingDisplayName,
  currentPassword,
  setCurrentPassword,
  newPassword,
  setNewPassword,
  confirmPassword,
  setConfirmPassword,
  savingPassword,
  onDisplayNameSubmit,
  onPasswordSubmit,
}: {
  displayName: string;
  setDisplayName: (value: string) => void;
  savingDisplayName: boolean;
  currentPassword: string;
  setCurrentPassword: (value: string) => void;
  newPassword: string;
  setNewPassword: (value: string) => void;
  confirmPassword: string;
  setConfirmPassword: (value: string) => void;
  savingPassword: boolean;
  onDisplayNameSubmit: (event: FormEvent<HTMLFormElement>) => void;
  onPasswordSubmit: (event: FormEvent<HTMLFormElement>) => void;
}) {
  return (
    <div className="space-y-5">
      <SectionHeader
        icon={<User className="size-4" aria-hidden />}
        title="账户"
        description="更新个人资料和登录凭据。"
      />
      <form className="space-y-3" onSubmit={onDisplayNameSubmit}>
        <SettingsField
          id="settings-display-name"
          label="显示名称"
          value={displayName}
          onChange={setDisplayName}
          autoComplete="name"
          required
        />
        <div className="flex justify-end">
          <Button type="submit" disabled={savingDisplayName}>
            {savingDisplayName ? (
              <Loader2 className="size-4 animate-spin" aria-hidden />
            ) : (
              <Save className="size-4" aria-hidden />
            )}
            保存名称
          </Button>
        </div>
      </form>
      <div className="h-px bg-border" />
      <form className="space-y-3" onSubmit={onPasswordSubmit}>
        <SettingsField
          id="settings-current-password"
          label="当前密码"
          type="password"
          value={currentPassword}
          onChange={setCurrentPassword}
          autoComplete="current-password"
          required
        />
        <div className="grid gap-3 sm:grid-cols-2">
          <SettingsField
            id="settings-new-password"
            label="新密码"
            type="password"
            value={newPassword}
            onChange={setNewPassword}
            autoComplete="new-password"
            required
          />
          <SettingsField
            id="settings-confirm-password"
            label="确认新密码"
            type="password"
            value={confirmPassword}
            onChange={setConfirmPassword}
            autoComplete="new-password"
            required
          />
        </div>
        <div className="flex justify-end">
          <Button type="submit" disabled={savingPassword}>
            {savingPassword ? (
              <Loader2 className="size-4 animate-spin" aria-hidden />
            ) : (
              <KeyRound className="size-4" aria-hidden />
            )}
            更新密码
          </Button>
        </div>
      </form>
    </div>
  );
}

function PreferencesTab({
  language,
  onLanguageChange,
  theme,
  onThemeChange,
}: {
  language: LanguagePreference;
  onLanguageChange: (language: LanguagePreference) => void;
  theme: ThemePreference;
  onThemeChange: (theme: ThemePreference) => void;
}) {
  return (
    <div className="space-y-5">
      <SectionHeader
        icon={<Monitor className="size-4" aria-hidden />}
        title="偏好"
        description="语言先保存偏好，主题会立即切换界面。"
      />
      <PreferenceGroup label="语言">
        <SegmentedOption
          active={language === "zh-CN"}
          label="中文"
          onClick={() => onLanguageChange("zh-CN")}
        />
        <SegmentedOption
          active={language === "en-US"}
          label="English"
          onClick={() => onLanguageChange("en-US")}
        />
      </PreferenceGroup>
      <PreferenceGroup label="主题">
        <IconOption
          active={theme === "system"}
          icon={<Monitor className="size-4" aria-hidden />}
          label="跟随系统"
          onClick={() => onThemeChange("system")}
        />
        <IconOption
          active={theme === "light"}
          icon={<Sun className="size-4" aria-hidden />}
          label="浅色"
          onClick={() => onThemeChange("light")}
        />
        <IconOption
          active={theme === "dark"}
          icon={<Moon className="size-4" aria-hidden />}
          label="深色"
          onClick={() => onThemeChange("dark")}
        />
      </PreferenceGroup>
    </div>
  );
}

function ModelTab({
  baseUrl,
  setBaseUrl,
  model,
  setModel,
  apiMode,
  setApiMode,
  apiKey,
  setApiKey,
  hasApiKey,
  isLoading,
  isSaving,
  onSubmit,
}: {
  baseUrl: string;
  setBaseUrl: (value: string) => void;
  model: string;
  setModel: (value: string) => void;
  apiMode: LlmProviderApiMode;
  setApiMode: (value: LlmProviderApiMode) => void;
  apiKey: string;
  setApiKey: (value: string) => void;
  hasApiKey: boolean;
  isLoading: boolean;
  isSaving: boolean;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
}) {
  return (
    <div className="space-y-5">
      <SectionHeader
        icon={<KeyRound className="size-4" aria-hidden />}
        title="模型"
        description="配置 OpenAI-compatible provider，用于 SQL 审计。"
      />
      {isLoading ? (
        <div className="flex items-center gap-2 rounded-lg border bg-muted/30 p-3 text-sm text-muted-foreground">
          <Loader2 className="size-4 animate-spin" aria-hidden />
          正在加载模型配置
        </div>
      ) : (
        <form className="space-y-3" onSubmit={onSubmit}>
          <div className="flex flex-wrap items-center gap-2">
            <Badge variant="secondary" className="rounded-md">
              OpenAI-compatible
            </Badge>
            {hasApiKey ? (
              <Badge variant="outline" className="rounded-md">
                <CheckCircle2 className="size-3" aria-hidden />
                API Key 已配置
              </Badge>
            ) : null}
          </div>
          <SettingsField
            id="settings-model-base-url"
            label="Base URL"
            value={baseUrl}
            onChange={setBaseUrl}
            placeholder="https://api.openai.com"
            required
          />
          <div className="grid gap-3 sm:grid-cols-[1fr_180px]">
            <SettingsField
              id="settings-model-name"
              label="模型"
              value={model}
              onChange={setModel}
              placeholder="gpt-4.1"
              required
            />
            <div className="space-y-1.5">
              <label
                className="text-xs font-medium text-muted-foreground"
                htmlFor="settings-api-mode"
              >
                API Mode
              </label>
              <select
                id="settings-api-mode"
                value={apiMode}
                onChange={(event) =>
                  setApiMode(event.target.value as LlmProviderApiMode)
                }
                className="h-9 w-full rounded-md border bg-background px-3 text-sm outline-none transition-shadow focus-visible:ring-[3px] focus-visible:ring-ring/50"
              >
                <option value="chat_completions">chat_completions</option>
                <option value="responses">responses</option>
              </select>
            </div>
          </div>
          <SettingsField
            id="settings-api-key"
            label="API Key"
            type="password"
            value={apiKey}
            onChange={setApiKey}
            placeholder={hasApiKey ? "留空则保持当前密钥" : ""}
            autoComplete="off"
          />
          <div className="flex justify-end">
            <Button type="submit" disabled={isSaving}>
              {isSaving ? (
                <Loader2 className="size-4 animate-spin" aria-hidden />
              ) : (
                <Save className="size-4" aria-hidden />
              )}
              保存模型配置
            </Button>
          </div>
        </form>
      )}
    </div>
  );
}

function SectionHeader({
  icon,
  title,
  description,
}: {
  icon: ReactNode;
  title: string;
  description: string;
}) {
  return (
    <div>
      <div className="flex items-center gap-2 text-sm font-semibold">
        <span className="text-muted-foreground">{icon}</span>
        {title}
      </div>
      <p className="mt-1 text-xs leading-5 text-muted-foreground">
        {description}
      </p>
    </div>
  );
}

function SettingsField({
  id,
  label,
  value,
  onChange,
  type = "text",
  placeholder,
  autoComplete,
  required = false,
}: {
  id: string;
  label: string;
  value: string;
  onChange: (value: string) => void;
  type?: string;
  placeholder?: string;
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
        placeholder={placeholder}
        autoComplete={autoComplete}
        required={required}
        className="h-9 w-full rounded-md border bg-background px-3 text-sm outline-none transition-shadow placeholder:text-muted-foreground focus-visible:ring-[3px] focus-visible:ring-ring/50"
      />
    </div>
  );
}

function PreferenceGroup({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) {
  return (
    <div className="space-y-2">
      <div className="text-xs font-medium text-muted-foreground">{label}</div>
      <div className="flex flex-wrap gap-2">{children}</div>
    </div>
  );
}

function SegmentedOption({
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
        "h-9 rounded-md border px-3 text-sm font-medium outline-none transition-colors focus-visible:ring-[3px] focus-visible:ring-ring/50",
        active
          ? "border-foreground/20 bg-foreground text-background"
          : "bg-background text-muted-foreground hover:bg-accent hover:text-foreground",
      )}
      onClick={onClick}
    >
      {label}
    </button>
  );
}

function IconOption({
  active,
  icon,
  label,
  onClick,
}: {
  active: boolean;
  icon: ReactNode;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      className={cn(
        "flex h-10 items-center gap-2 rounded-md border px-3 text-sm font-medium outline-none transition-colors focus-visible:ring-[3px] focus-visible:ring-ring/50",
        active
          ? "border-foreground/20 bg-foreground text-background"
          : "bg-background text-muted-foreground hover:bg-accent hover:text-foreground",
      )}
      onClick={onClick}
    >
      {icon}
      {label}
    </button>
  );
}
