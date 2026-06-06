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
  type LucideIcon,
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

const tabs: Array<{
  value: SettingsTab;
  label: string;
  description: string;
  icon: LucideIcon;
}> = [
  { value: "account", label: "账户", description: "资料与密码", icon: User },
  { value: "preferences", label: "偏好", description: "语言与主题", icon: Monitor },
  { value: "model", label: "模型", description: "LLM Provider", icon: KeyRound },
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
  const [baseUrl, setBaseUrl] = useState(
    "https://api.openai.com/v1/chat/completions",
  );
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

  const userInitials =
    user.display_name
      .split(/\s+/)
      .filter(Boolean)
      .slice(0, 2)
      .map((part) => part[0])
      .join("")
      .toUpperCase() || user.email.slice(0, 1).toUpperCase();

  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center overflow-y-auto bg-background/70 p-3 backdrop-blur-sm sm:items-center">
      <button
        type="button"
        className="absolute inset-0 cursor-default"
        aria-label="关闭设置弹窗"
        onClick={onClose}
      />
      <Card
        className="relative w-full max-w-[860px] animate-in fade-in-0 zoom-in-95 overflow-hidden rounded-xl border-border/80 bg-card/95 py-0 shadow-2xl shadow-foreground/10 duration-150"
        role="dialog"
        aria-modal="true"
        aria-labelledby="account-settings-title"
      >
        <CardHeader className="flex flex-row items-center justify-between gap-4 border-b bg-muted/20 px-4 py-4 sm:px-5">
          <div className="flex min-w-0 items-center gap-3">
            <div className="flex size-10 shrink-0 items-center justify-center rounded-lg border bg-background shadow-xs">
              <Settings className="size-4 text-muted-foreground" aria-hidden />
            </div>
            <div className="min-w-0">
              <CardTitle
                id="account-settings-title"
                className="text-base font-semibold"
              >
                设置
              </CardTitle>
              <div className="mt-1 flex min-w-0 items-center gap-2 text-xs text-muted-foreground">
                <span className="flex size-5 shrink-0 items-center justify-center rounded-full bg-primary text-[10px] font-semibold text-primary-foreground">
                  {userInitials}
                </span>
                <span className="truncate">{user.display_name}</span>
                <span className="text-border" aria-hidden>
                  /
                </span>
                <span className="truncate">{user.email}</span>
              </div>
            </div>
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
        <CardContent className="grid max-h-[calc(100vh-1.5rem-73px)] gap-0 overflow-hidden p-0 sm:grid-cols-[210px_1fr]">
          <div
            className="flex gap-2 overflow-x-auto border-b bg-muted/30 p-3 sm:flex-col sm:border-b-0 sm:border-r sm:bg-muted/20 sm:p-4"
            role="tablist"
            aria-label="设置分类"
          >
            {tabs.map((tab) => {
              const Icon = tab.icon;

              return (
                <button
                  key={tab.value}
                  type="button"
                  id={`${tab.value}-settings-tab`}
                  role="tab"
                  aria-selected={activeTab === tab.value}
                  aria-controls={`${tab.value}-settings-panel`}
                  className={cn(
                    "group flex h-11 shrink-0 items-center gap-2.5 rounded-lg border border-transparent px-3 text-left outline-none transition-all focus-visible:ring-[3px] focus-visible:ring-ring/50 sm:h-auto sm:py-2.5",
                    activeTab === tab.value
                      ? "border-border bg-background text-foreground shadow-sm"
                      : "text-muted-foreground hover:border-border/70 hover:bg-background/60 hover:text-foreground",
                  )}
                  onClick={() => setActiveTab(tab.value)}
                >
                  <span
                    className={cn(
                      "flex size-7 shrink-0 items-center justify-center rounded-md border transition-colors",
                      activeTab === tab.value
                        ? "bg-primary text-primary-foreground"
                        : "bg-background text-muted-foreground group-hover:text-foreground",
                    )}
                  >
                    <Icon className="size-3.5" aria-hidden />
                  </span>
                  <span className="min-w-0">
                    <span className="block text-sm font-medium leading-none">
                      {tab.label}
                    </span>
                    <span className="mt-1 hidden truncate text-xs text-muted-foreground sm:block">
                      {tab.description}
                    </span>
                  </span>
                </button>
              );
            })}
          </div>
          <div className="min-h-[440px] overflow-y-auto p-4 sm:p-5">
            {activeTab === "account" ? (
              <SettingsPanel
                id="account-settings-panel"
                labelledBy="account-settings-tab"
              >
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
              <SettingsPanel
                id="model-settings-panel"
                labelledBy="model-settings-tab"
              >
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
    <div className="space-y-4">
      <SectionHeader
        icon={<User className="size-4" aria-hidden />}
        title="账户"
        description="更新个人资料和登录凭据。"
        badge="本地账户"
      />

      <SettingsBlock title="个人资料" description="显示名称会同步到账户区。">
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
      </SettingsBlock>

      <SettingsBlock title="登录密码" description="修改后下次登录使用新密码。">
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
      </SettingsBlock>
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
    <div className="space-y-4">
      <SectionHeader
        icon={<Monitor className="size-4" aria-hidden />}
        title="偏好"
        description="语言先保存偏好，主题会立即切换界面。"
        badge={theme === "system" ? "跟随系统" : theme === "dark" ? "深色" : "浅色"}
      />

      <SettingsBlock title="语言" description="当前仅保存偏好，不改变全站文案。">
        <PreferenceGroup>
          <SegmentedOption
            active={language === "zh-CN"}
            label="中文"
            description="zh-CN"
            onClick={() => onLanguageChange("zh-CN")}
          />
          <SegmentedOption
            active={language === "en-US"}
            label="English"
            description="en-US"
            onClick={() => onLanguageChange("en-US")}
          />
        </PreferenceGroup>
      </SettingsBlock>

      <SettingsBlock title="主题" description="立即切换当前浏览器中的界面主题。">
        <PreferenceGroup>
          <IconOption
            active={theme === "system"}
            icon={<Monitor className="size-4" aria-hidden />}
            label="跟随系统"
            description="使用系统外观"
            onClick={() => onThemeChange("system")}
          />
          <IconOption
            active={theme === "light"}
            icon={<Sun className="size-4" aria-hidden />}
            label="浅色"
            description="明亮工作区"
            onClick={() => onThemeChange("light")}
          />
          <IconOption
            active={theme === "dark"}
            icon={<Moon className="size-4" aria-hidden />}
            label="深色"
            description="低亮界面"
            onClick={() => onThemeChange("dark")}
          />
        </PreferenceGroup>
      </SettingsBlock>
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
    <div className="space-y-4">
      <SectionHeader
        icon={<KeyRound className="size-4" aria-hidden />}
        title="模型"
        description="配置 OpenAI-compatible provider，用于 SQL 审计。"
        badge={hasApiKey ? "API Key 已配置" : "待配置"}
      />
      {isLoading ? (
        <div className="flex items-center gap-2 rounded-lg border bg-muted/30 p-4 text-sm text-muted-foreground shadow-xs">
          <Loader2 className="size-4 animate-spin" aria-hidden />
          正在加载模型配置
        </div>
      ) : (
        <form className="space-y-4" onSubmit={onSubmit}>
          <div className="rounded-lg border bg-muted/25 p-3 shadow-xs">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <div className="flex items-center gap-3">
                <div className="flex size-9 items-center justify-center rounded-lg border bg-background">
                  <KeyRound className="size-4 text-muted-foreground" aria-hidden />
                </div>
                <div>
                  <div className="text-sm font-medium">OpenAI-compatible</div>
                  <div className="mt-0.5 text-xs text-muted-foreground">
                    SQL 审计会优先使用当前账户的模型配置。
                  </div>
                </div>
              </div>
              <Badge
                variant={hasApiKey ? "secondary" : "outline"}
                className="rounded-md"
              >
                {hasApiKey ? (
                  <CheckCircle2 className="size-3" aria-hidden />
                ) : null}
                {hasApiKey ? "API Key 已配置" : "API Key 未配置"}
              </Badge>
            </div>
          </div>

          <SettingsBlock title="接口" description="填写完整 endpoint URL，并选择匹配的 API Mode。">
            <SettingsField
              id="settings-model-base-url"
              label="完整 URL"
              value={baseUrl}
              onChange={setBaseUrl}
              placeholder="https://api.openai.com/v1/chat/completions"
              helpText="例如 /v1/chat/completions 或 /v1/responses。"
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
                  className="h-9 w-full rounded-md border bg-background px-3 text-sm shadow-xs outline-none transition-shadow focus-visible:ring-[3px] focus-visible:ring-ring/50"
                >
                  <option value="chat_completions">chat_completions</option>
                  <option value="responses">responses</option>
                </select>
              </div>
            </div>
          </SettingsBlock>

          <SettingsBlock
            title="密钥"
            description={
              hasApiKey ? "留空保存时会继续使用已保存密钥。" : "API Key 只会加密保存，不会回显。"
            }
          >
            <SettingsField
              id="settings-api-key"
              label="API Key"
              type="password"
              value={apiKey}
              onChange={setApiKey}
              placeholder={hasApiKey ? "留空则保持当前密钥" : ""}
              autoComplete="off"
            />
          </SettingsBlock>

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
  badge,
}: {
  icon: ReactNode;
  title: string;
  description: string;
  badge?: string;
}) {
  return (
    <div className="flex flex-wrap items-start justify-between gap-3">
      <div className="min-w-0">
        <div className="flex items-center gap-2 text-sm font-semibold">
          <span className="flex size-8 items-center justify-center rounded-lg border bg-muted/40 text-muted-foreground">
            {icon}
          </span>
          <span>{title}</span>
        </div>
        <p className="mt-2 text-xs leading-5 text-muted-foreground">
          {description}
        </p>
      </div>
      {badge ? (
        <Badge variant="outline" className="rounded-md bg-background">
          {badge}
        </Badge>
      ) : null}
    </div>
  );
}

function SettingsBlock({
  title,
  description,
  children,
}: {
  title: string;
  description: string;
  children: ReactNode;
}) {
  return (
    <section className="rounded-lg border bg-background p-3 shadow-xs sm:p-4">
      <div className="mb-3">
        <h3 className="text-sm font-medium">{title}</h3>
        <p className="mt-1 text-xs leading-5 text-muted-foreground">
          {description}
        </p>
      </div>
      <div className="space-y-3">{children}</div>
    </section>
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
  helpText,
}: {
  id: string;
  label: string;
  value: string;
  onChange: (value: string) => void;
  type?: string;
  placeholder?: string;
  autoComplete?: string;
  required?: boolean;
  helpText?: string;
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
        className="h-9 w-full rounded-md border bg-background px-3 text-sm shadow-xs outline-none transition-shadow placeholder:text-muted-foreground focus-visible:ring-[3px] focus-visible:ring-ring/50"
      />
      {helpText ? (
        <p className="text-xs leading-5 text-muted-foreground">{helpText}</p>
      ) : null}
    </div>
  );
}

function PreferenceGroup({
  children,
}: {
  children: ReactNode;
}) {
  return (
    <div className="grid gap-2 sm:grid-cols-2">{children}</div>
  );
}

function SegmentedOption({
  active,
  label,
  description,
  onClick,
}: {
  active: boolean;
  label: string;
  description: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      className={cn(
        "relative flex min-h-16 flex-col items-start justify-center rounded-lg border px-3 py-2 pr-9 text-left outline-none transition-all focus-visible:ring-[3px] focus-visible:ring-ring/50",
        active
          ? "border-primary/40 bg-primary/5 text-foreground shadow-sm ring-1 ring-primary/10"
          : "bg-muted/20 text-muted-foreground hover:border-foreground/20 hover:bg-muted/40 hover:text-foreground",
      )}
      onClick={onClick}
    >
      {active ? (
        <CheckCircle2
          className="absolute right-3 top-3 size-3.5 text-primary"
          aria-hidden
        />
      ) : null}
      <span className="text-sm font-medium">{label}</span>
      <span className="mt-1 text-xs text-muted-foreground">{description}</span>
    </button>
  );
}

function IconOption({
  active,
  icon,
  label,
  description,
  onClick,
}: {
  active: boolean;
  icon: ReactNode;
  label: string;
  description: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      className={cn(
        "relative flex min-h-16 items-center gap-3 rounded-lg border px-3 py-2 pr-9 text-left outline-none transition-all focus-visible:ring-[3px] focus-visible:ring-ring/50",
        active
          ? "border-primary/40 bg-primary/5 text-foreground shadow-sm ring-1 ring-primary/10"
          : "bg-muted/20 text-muted-foreground hover:border-foreground/20 hover:bg-muted/40 hover:text-foreground",
      )}
      onClick={onClick}
    >
      {active ? (
        <CheckCircle2
          className="absolute right-3 top-3 size-3.5 text-primary"
          aria-hidden
        />
      ) : null}
      <span
        className={cn(
          "flex size-8 shrink-0 items-center justify-center rounded-md border",
          active
            ? "border-primary/30 bg-primary text-primary-foreground"
            : "bg-background",
        )}
      >
        {icon}
      </span>
      <span className="min-w-0">
        <span className="block text-sm font-medium">{label}</span>
        <span className="mt-0.5 block text-xs text-muted-foreground">
          {description}
        </span>
      </span>
    </button>
  );
}
