"use client";

import {
  createContext,
  type ReactNode,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";

export const LANGUAGE_STORAGE_KEY = "liquid.preferences.language";
export const DEFAULT_LOCALE = "zh-CN";

export const localeOptions = [
  { value: "zh-CN", label: "中文", description: "zh-CN" },
  { value: "en-US", label: "English", description: "en-US" },
] as const;

export type Locale = (typeof localeOptions)[number]["value"];

const zhCN = {
  common: {
    cancel: "取消",
    close: "关闭",
    delete: "删除",
    save: "保存",
    search: "搜索",
  },
  auth: {
    loadingTitle: "正在恢复会话",
    loadingDescription: "Liquid 正在校验本地令牌",
    subtitle: "登录后管理托管数据库与 SQL 风险看板",
    status: {
      session: "会话",
      credential: "凭据",
      database: "数据库",
      encryptedStorage: "加密存储",
    },
    managedByLiquid: "托管数据库由 Liquid 管理",
    managedDescription:
      "连接信息保存在 Liquid 应用库中。第一版只管理连接记录，不自动同步 schema 元数据。",
    loginTitle: "登录账户",
    registerTitle: "注册账户",
    loginBadge: "安全会话",
    registerBadge: "开放注册",
    loginTab: "登录",
    registerTab: "注册",
    email: "邮箱",
    displayName: "显示名称",
    password: "密码",
    createAccount: "创建账户",
    login: "登录",
    errors: {
      failed: "认证失败",
    },
  },
  databasePicker: {
    loadFailed: "加载托管数据库失败",
    portRangeError: "端口必须是 1-65535 的数字",
    updated: "连接记录已更新",
    created: "连接记录已创建",
    saveFailed: "保存数据库失败",
    deleteConfirm: (name: string) => `删除连接「${name}」？`,
    deleted: "连接记录已删除",
    deleteFailed: "删除数据库失败",
    testFailed: "连接测试失败",
    enterFailed: "进入工作区失败",
    title: "工作区概览",
    currentAccount: "当前账户",
    accountMenuLabel: (name: string) => `${name} 账户菜单`,
    settings: "设置",
    logout: "注销",
    workspaceTitle: "数据库工作区",
    connectionCount: (count: number) => `${count} 个连接记录`,
    addConnection: "新增连接",
    searchPlaceholder: "搜索名称、主机、数据库",
    loadingConnections: "正在加载连接记录",
    noMatches: "没有匹配的数据库",
    passwordSaved: "密码已保存",
    noPassword: "无密码",
    connectionAvailable: "连接可用",
    connectionIssue: "连接异常",
    enter: "进入",
    testConnection: "测试连接",
    testDatabaseLabel: (name: string) => `测试 ${name}`,
    editConnection: "编辑连接",
    editDatabaseLabel: (name: string) => `编辑 ${name}`,
    deleteConnection: "删除连接",
    deleteDatabaseLabel: (name: string) => `删除 ${name}`,
    closeDialog: "关闭弹窗",
    editDialogTitle: "编辑数据库连接",
    createDialogTitle: "新增数据库连接",
    encryptedDescription: "连接信息会加密保存",
    fields: {
      name: "名称",
      host: "主机",
      port: "端口",
      database: "数据库",
      username: "用户名",
      newPassword: "新密码",
      password: "密码",
      keepPasswordPlaceholder: "留空则保持原密码",
    },
    saveChanges: "保存修改",
    saveConnection: "保存连接",
    emptyTitle: "暂无托管数据库",
    emptyDescription: "使用顶部新增连接创建记录后才能进入 SQL 风险工作区。",
  },
  settings: {
    tabs: {
      account: {
        label: "账户",
        description: "资料与密码",
      },
      preferences: {
        label: "偏好",
        description: "语言与主题",
      },
      model: {
        label: "模型",
        description: "LLM Provider",
      },
    },
    dialog: {
      title: "设置",
      closeDialog: "关闭设置弹窗",
      categoriesLabel: "设置分类",
    },
    toasts: {
      loadModelFailed: "加载模型配置失败",
      displayNameUpdated: "显示名称已更新",
      displayNameUpdateFailed: "更新显示名称失败",
      passwordMismatch: "两次输入的新密码不一致",
      passwordUpdated: "密码已更新",
      passwordUpdateFailed: "更新密码失败",
      languageSaved: "语言偏好已保存",
      modelSaved: "模型配置已保存",
      modelSaveFailed: "保存模型配置失败",
    },
    account: {
      title: "账户",
      description: "更新个人资料和登录凭据。",
      badge: "本地账户",
      profileTitle: "个人资料",
      profileDescription: "显示名称会同步到账户区。",
      displayName: "显示名称",
      saveName: "保存名称",
      passwordTitle: "登录密码",
      passwordDescription: "修改后下次登录使用新密码。",
      currentPassword: "当前密码",
      newPassword: "新密码",
      confirmPassword: "确认新密码",
      updatePassword: "更新密码",
    },
    preferences: {
      title: "偏好",
      description: "语言和主题会立即切换当前浏览器中的界面。",
      languageTitle: "语言",
      languageDescription: "切换后全站文案会立即更新。",
      themeTitle: "主题",
      themeDescription: "立即切换当前浏览器中的界面主题。",
      themeBadges: {
        system: "跟随系统",
        dark: "深色",
        light: "浅色",
      },
      themeOptions: {
        system: {
          label: "跟随系统",
          description: "使用系统外观",
        },
        light: {
          label: "浅色",
          description: "明亮工作区",
        },
        dark: {
          label: "深色",
          description: "低亮界面",
        },
      },
    },
    model: {
      title: "模型",
      description: "配置 OpenAI-compatible provider，用于 SQL 审计和 AI 工作台聊天。",
      configuredBadge: "API Key 已配置",
      pendingBadge: "待配置",
      loading: "正在加载模型配置",
      providerDescription: "SQL 审计和 AI 工作台聊天会优先使用当前账户的模型配置。",
      apiKeyConfigured: "API Key 已配置",
      apiKeyMissing: "API Key 未配置",
      endpointTitle: "接口",
      endpointDescription: "填写完整 endpoint URL，并选择匹配的 API Mode。",
      fullUrl: "完整 URL",
      urlHelp: "例如 /v1/chat/completions 或 /v1/responses。",
      modelLabel: "模型",
      secretTitle: "密钥",
      secretDescriptionConfigured: "留空保存时会继续使用已保存密钥。",
      secretDescriptionMissing: "API Key 只会加密保存，不会回显。",
      keepSecretPlaceholder: "留空则保持当前密钥",
      save: "保存模型配置",
    },
  },
  workspace: {
    defaultTitlePrefix: "AI工作区",
    loadFailed: "加载工作区失败",
    createFailed: "新建工作区失败",
    deleteFailed: "删除工作区失败",
    loadingWorkspace: "正在加载工作区",
    newWorkspace: "新建工作区",
    returnToDatabases: "返回数据库选择",
    agentLoadFailed: "加载 agent 工作台失败",
    renameFailed: "工作区重命名失败",
    sendFailed: "发送失败",
    providerNotConfigured:
      "请先在设置中配置 LLM Provider 和 API Key，再使用 AI 工作台聊天。",
    providerReady: "模型已连接",
    providerMissing: "模型未配置",
    providerSetupHint:
      "当前账户还没有可用 API Key。配置 LLM Provider 后即可开始真实聊天。",
    errorMessages: {
      provider_not_configured:
        "请先在设置中配置 LLM Provider 和 API Key，再使用 AI 工作台聊天。",
      provider_request_failed: "模型请求失败，请检查 provider 地址、密钥和模型名称。",
      invalid_model_response: "模型返回内容格式无效，请重试或调整模型配置。",
      invalid_action_intent: "模型提出的动作不符合当前会话约束，已阻止执行。",
      storage_error: "会话数据读写失败，请稍后重试。",
      turn_cancelled: "本次回复已停止。",
    },
    actionFailed: "动作处理失败",
    actionApplied: "动作已确认",
    actionRejected: "动作已拒绝",
    introMessage: (databaseName: string) =>
      `当前绑定 ${databaseName}，可以直接发送 SQL 或治理问题。敏感操作会先变成待确认动作。`,
    workspaceName: "工作区名称",
    deleteWorkspaceLabel: (title: string) => `删除工作区 ${title}`,
    deleteWorkspaceTitle: "删除工作区",
    loadingConversation: "正在加载会话",
    quickPrompts: [
      "解释风险上升原因",
      "生成周报摘要",
      "找出异常数据集",
      "给出治理建议",
    ],
    emptyTitle: "开始一次数据库对话",
    emptyDescription: (databaseName: string) =>
      `当前绑定 ${databaseName}。发送 SQL、审计问题或治理请求，Liquid 会把需要确认的操作变成动作卡。`,
    inputLabel: "输入问题",
    inputPlaceholder: "输入问题，例如：列出本周风险最高的数据集",
    composerHint: "Enter 发送，Shift+Enter 换行",
    sendQuestion: "发送问题",
    send: "发送",
    stop: "停止",
    retry: "重试",
    copy: "复制",
    copied: "已复制",
    copyFailed: "复制失败",
    copyCode: "复制代码",
    codeBlock: "代码",
    deleteConfirm: (title: string) =>
      `确认删除「${title}」？该工作区的会话内容会一并移除。`,
    pending: "发送中",
    localPending: "本地",
    messageFailed: "回复失败",
    userLabel: "你",
    assistantLabel: "Liquid",
    toolLabel: "操作结果",
    sqlPreview: "SQL 审计预览",
    actionProcessing: "处理中",
    actionStatusUpdated: "动作状态已更新",
    stages: {
      thinking: "正在思考",
      loading_context: "正在加载上下文",
      proposing_action: "正在准备动作",
    },
    confirm: "确认",
    confirming: "确认中",
    importToBiPanel: "导入 BI 面板",
    importingToBiPanel: "导入中",
    reject: "拒绝",
    rejecting: "拒绝中",
    biPreview: "BI 卡片预览",
    biRows: (count: number) => `${count} 行`,
    actionLabels: {
      create_sql_audit: "创建审计",
      create_bi_card: "创建 BI 卡片",
      approve_sql_audit: "批准审计",
      reject_sql_audit: "拒绝审计",
      execute_sql_audit: "执行审计",
      create_managed_database: "新增数据库",
      update_managed_database: "更新数据库",
      delete_managed_database: "删除数据库",
      start_database_backup: "备份",
      start_database_restore: "恢复",
    },
    actionStatuses: {
      proposed: "待确认",
      applied: "已执行",
      rejected: "已拒绝",
      failed: "失败",
      superseded: "已替换",
    },
    splitHandleLabel: "拖拽调整 AI 与 BI 区域宽度",
    splitHandleTitle: "拖拽调整宽度，双击恢复默认",
  },
  dashboard: {
    title: "BI 数据看板",
    export: "导出",
    allDatasets: "全部数据集",
    loading: "正在加载",
    loadFailed: "加载 BI 面板失败",
    saved: "已保存",
    saveFailed: "保存失败",
    layoutSaveFailed: "布局保存失败",
    refreshed: "数据已刷新",
    refreshFailed: "刷新失败",
    deleted: "卡片已删除",
    deleteFailed: "删除卡片失败",
    exported: "面板 JSON 已导出",
    exportFailed: "导出失败",
    panelTitleLabel: "面板名称",
    panelDescriptionLabel: "面板描述",
    cardTitleLabel: "卡片名称",
    cardDescriptionLabel: "卡片描述",
    cardCount: (count: number) => `${count} 张卡片`,
    emptyTitle: "BI 面板暂无卡片",
    emptyDescription: "在左侧向 AI 请求表格或图表，然后确认动作导入到这里。",
    refresh: "刷新数据",
    editCard: "编辑卡片",
    deleteCard: "删除卡片",
    sqlSource: (count: number) => `${count} 行快照`,
    cardKinds: {
      table: "表格",
      chart: "图表",
    },
    metrics: {
      totalQueries: "查询总量",
      passRate: "通过率",
      riskEvents: "风险事件",
      pendingReview: "待复核",
      highRisk: "高风险",
    },
    weekdays: {
      monday: "周一",
      tuesday: "周二",
      wednesday: "周三",
      thursday: "周四",
      friday: "周五",
      saturday: "周六",
      sunday: "周日",
    },
    chartKeys: {
      queries: "查询量",
      risks: "风险量",
      passRate: "通过率",
      count: "数量",
    },
    categories: {
      customerProfile: "客户画像",
      orderFacts: "订单流水",
      accessAudit: "权限审计",
      marketing: "营销分析",
      inventory: "库存同步",
    },
    owners: {
      dataGovernance: "数据治理组",
      transactionPlatform: "交易平台",
      securityPlatform: "安全平台",
      growthAnalytics: "增长分析",
      supplyChain: "供应链",
    },
    updatedAt: {
      tenMinutes: "10 分钟前",
      eighteenMinutes: "18 分钟前",
      twentyFourMinutes: "24 分钟前",
      fortyTwoMinutes: "42 分钟前",
      oneHour: "1 小时前",
    },
    riskLevels: {
      low: "低",
      medium: "中",
      high: "高",
      critical: "严重",
    },
    datasetStatuses: {
      normal: "正常",
      watch: "观察",
      needsAction: "需处理",
    },
    trendTitle: "查询趋势",
    trendDescription: "查询量、风险量和通过率变化",
    riskDistributionTitle: "风险分布",
    riskDistributionDescription: "按数据域聚合的风险事件",
    datasetTableTitle: "数据集明细",
    datasetTableDescription: "模拟数据，按风险优先级展示",
    tableHeaders: {
      dataset: "数据集",
      owner: "负责人",
      queries: "查询量",
      risk: "风险级别",
      status: "状态",
      updatedAt: "更新时间",
    },
  },
};

type Messages = typeof zhCN;

const enUS: Messages = {
  common: {
    cancel: "Cancel",
    close: "Close",
    delete: "Delete",
    save: "Save",
    search: "Search",
  },
  auth: {
    loadingTitle: "Restoring session",
    loadingDescription: "Liquid is validating the local token",
    subtitle: "Sign in to manage databases and the SQL risk dashboard",
    status: {
      session: "Session",
      credential: "Credential",
      database: "Database",
      encryptedStorage: "Encrypted storage",
    },
    managedByLiquid: "Managed databases are handled by Liquid",
    managedDescription:
      "Connection details are stored in the Liquid application database. This first version only manages connection records and does not automatically sync schema metadata.",
    loginTitle: "Sign in",
    registerTitle: "Create account",
    loginBadge: "Secure session",
    registerBadge: "Open registration",
    loginTab: "Sign in",
    registerTab: "Register",
    email: "Email",
    displayName: "Display name",
    password: "Password",
    createAccount: "Create account",
    login: "Sign in",
    errors: {
      failed: "Authentication failed",
    },
  },
  databasePicker: {
    loadFailed: "Failed to load managed databases",
    portRangeError: "Port must be a number from 1 to 65535",
    updated: "Connection updated",
    created: "Connection created",
    saveFailed: "Failed to save database",
    deleteConfirm: (name: string) => `Delete connection "${name}"?`,
    deleted: "Connection deleted",
    deleteFailed: "Failed to delete database",
    testFailed: "Connection test failed",
    enterFailed: "Failed to enter workspace",
    title: "Workspace overview",
    currentAccount: "Current account",
    accountMenuLabel: (name: string) => `${name} account menu`,
    settings: "Settings",
    logout: "Log out",
    workspaceTitle: "Database workspace",
    connectionCount: (count: number) =>
      `${count} connection ${count === 1 ? "record" : "records"}`,
    addConnection: "Add connection",
    searchPlaceholder: "Search name, host, or database",
    loadingConnections: "Loading connection records",
    noMatches: "No matching databases",
    passwordSaved: "Password saved",
    noPassword: "No password",
    connectionAvailable: "Connection available",
    connectionIssue: "Connection issue",
    enter: "Enter",
    testConnection: "Test connection",
    testDatabaseLabel: (name: string) => `Test ${name}`,
    editConnection: "Edit connection",
    editDatabaseLabel: (name: string) => `Edit ${name}`,
    deleteConnection: "Delete connection",
    deleteDatabaseLabel: (name: string) => `Delete ${name}`,
    closeDialog: "Close dialog",
    editDialogTitle: "Edit database connection",
    createDialogTitle: "Add database connection",
    encryptedDescription: "Connection details are encrypted at rest",
    fields: {
      name: "Name",
      host: "Host",
      port: "Port",
      database: "Database",
      username: "Username",
      newPassword: "New password",
      password: "Password",
      keepPasswordPlaceholder: "Leave blank to keep the current password",
    },
    saveChanges: "Save changes",
    saveConnection: "Save connection",
    emptyTitle: "No managed databases",
    emptyDescription:
      "Use Add connection at the top to create a record before entering the SQL risk workspace.",
  },
  settings: {
    tabs: {
      account: {
        label: "Account",
        description: "Profile and password",
      },
      preferences: {
        label: "Preferences",
        description: "Language and theme",
      },
      model: {
        label: "Model",
        description: "LLM Provider",
      },
    },
    dialog: {
      title: "Settings",
      closeDialog: "Close settings dialog",
      categoriesLabel: "Settings categories",
    },
    toasts: {
      loadModelFailed: "Failed to load model settings",
      displayNameUpdated: "Display name updated",
      displayNameUpdateFailed: "Failed to update display name",
      passwordMismatch: "The new passwords do not match",
      passwordUpdated: "Password updated",
      passwordUpdateFailed: "Failed to update password",
      languageSaved: "Language preference saved",
      modelSaved: "Model settings saved",
      modelSaveFailed: "Failed to save model settings",
    },
    account: {
      title: "Account",
      description: "Update your profile and login credentials.",
      badge: "Local account",
      profileTitle: "Profile",
      profileDescription: "The display name is shown in account areas.",
      displayName: "Display name",
      saveName: "Save name",
      passwordTitle: "Login password",
      passwordDescription: "Use the new password the next time you sign in.",
      currentPassword: "Current password",
      newPassword: "New password",
      confirmPassword: "Confirm new password",
      updatePassword: "Update password",
    },
    preferences: {
      title: "Preferences",
      description:
        "Language and theme changes apply immediately in this browser.",
      languageTitle: "Language",
      languageDescription: "Switching updates application copy immediately.",
      themeTitle: "Theme",
      themeDescription: "Change the interface theme in this browser.",
      themeBadges: {
        system: "System",
        dark: "Dark",
        light: "Light",
      },
      themeOptions: {
        system: {
          label: "System",
          description: "Use system appearance",
        },
        light: {
          label: "Light",
          description: "Bright workspace",
        },
        dark: {
          label: "Dark",
          description: "Low-light interface",
        },
      },
    },
    model: {
      title: "Model",
      description:
        "Configure an OpenAI-compatible provider for SQL audits and AI workbench chat.",
      configuredBadge: "API key configured",
      pendingBadge: "Needs setup",
      loading: "Loading model settings",
      providerDescription:
        "SQL audits and AI workbench chat prefer the current account's model settings.",
      apiKeyConfigured: "API key configured",
      apiKeyMissing: "API key missing",
      endpointTitle: "Endpoint",
      endpointDescription:
        "Enter the full endpoint URL and choose the matching API mode.",
      fullUrl: "Full URL",
      urlHelp: "For example /v1/chat/completions or /v1/responses.",
      modelLabel: "Model",
      secretTitle: "Secret",
      secretDescriptionConfigured:
        "Leave blank when saving to keep the stored key.",
      secretDescriptionMissing:
        "The API key is encrypted at rest and never displayed again.",
      keepSecretPlaceholder: "Leave blank to keep the current key",
      save: "Save model settings",
    },
  },
  workspace: {
    defaultTitlePrefix: "AI Workspace",
    loadFailed: "Failed to load workspaces",
    createFailed: "Failed to create workspace",
    deleteFailed: "Failed to delete workspace",
    loadingWorkspace: "Loading workspace",
    newWorkspace: "New workspace",
    returnToDatabases: "Back to database selection",
    agentLoadFailed: "Failed to load agent workbench",
    renameFailed: "Failed to rename workspace",
    sendFailed: "Failed to send",
    providerNotConfigured:
      "Configure an LLM provider and API key in Settings before using AI workbench chat.",
    providerReady: "Model connected",
    providerMissing: "Model missing",
    providerSetupHint:
      "This account does not have a usable API key yet. Configure an LLM provider to start real chat.",
    errorMessages: {
      provider_not_configured:
        "Configure an LLM provider and API key in Settings before using AI workbench chat.",
      provider_request_failed:
        "The model request failed. Check the provider URL, API key, and model name.",
      invalid_model_response:
        "The model returned an invalid response. Retry or adjust the model settings.",
      invalid_action_intent:
        "The model proposed an action outside this conversation's constraints, so it was blocked.",
      storage_error: "Failed to read or write conversation data. Try again later.",
      turn_cancelled: "This response was stopped.",
    },
    actionFailed: "Failed to process action",
    actionApplied: "Action confirmed",
    actionRejected: "Action rejected",
    introMessage: (databaseName: string) =>
      `Currently connected to ${databaseName}. Send SQL or governance questions directly. Sensitive operations become confirmation actions first.`,
    workspaceName: "Workspace name",
    deleteWorkspaceLabel: (title: string) => `Delete workspace ${title}`,
    deleteWorkspaceTitle: "Delete workspace",
    loadingConversation: "Loading conversation",
    quickPrompts: [
      "Explain the risk increase",
      "Draft a weekly summary",
      "Find anomalous datasets",
      "Suggest governance actions",
    ],
    emptyTitle: "Start a database chat",
    emptyDescription: (databaseName: string) =>
      `Currently bound to ${databaseName}. Send SQL, audit questions, or governance requests; Liquid will turn sensitive operations into confirmation cards.`,
    inputLabel: "Question",
    inputPlaceholder:
      "Ask a question, for example: list the highest-risk datasets this week",
    composerHint: "Enter to send, Shift+Enter for a new line",
    sendQuestion: "Send question",
    send: "Send",
    stop: "Stop",
    retry: "Retry",
    copy: "Copy",
    copied: "Copied",
    copyFailed: "Copy failed",
    copyCode: "Copy code",
    codeBlock: "Code",
    deleteConfirm: (title: string) =>
      `Delete "${title}"? Its conversation history will be removed as well.`,
    pending: "Sending",
    localPending: "Local",
    messageFailed: "Response failed",
    userLabel: "You",
    assistantLabel: "Liquid",
    toolLabel: "Action result",
    sqlPreview: "SQL audit preview",
    actionProcessing: "Processing",
    actionStatusUpdated: "Action status updated",
    stages: {
      thinking: "Thinking",
      loading_context: "Loading context",
      proposing_action: "Preparing action",
    },
    confirm: "Confirm",
    confirming: "Confirming",
    importToBiPanel: "Import to BI panel",
    importingToBiPanel: "Importing",
    reject: "Reject",
    rejecting: "Rejecting",
    biPreview: "BI card preview",
    biRows: (count: number) => `${count} ${count === 1 ? "row" : "rows"}`,
    actionLabels: {
      create_sql_audit: "Create audit",
      create_bi_card: "Create BI card",
      approve_sql_audit: "Approve audit",
      reject_sql_audit: "Reject audit",
      execute_sql_audit: "Execute audit",
      create_managed_database: "Add database",
      update_managed_database: "Update database",
      delete_managed_database: "Delete database",
      start_database_backup: "Backup",
      start_database_restore: "Restore",
    },
    actionStatuses: {
      proposed: "Proposed",
      applied: "Applied",
      rejected: "Rejected",
      failed: "Failed",
      superseded: "Superseded",
    },
    splitHandleLabel: "Drag to resize AI and BI panes",
    splitHandleTitle: "Drag to resize, double-click to reset",
  },
  dashboard: {
    title: "BI Dashboard",
    export: "Export",
    allDatasets: "All datasets",
    loading: "Loading",
    loadFailed: "Failed to load BI panel",
    saved: "Saved",
    saveFailed: "Save failed",
    layoutSaveFailed: "Failed to save layout",
    refreshed: "Data refreshed",
    refreshFailed: "Refresh failed",
    deleted: "Card deleted",
    deleteFailed: "Failed to delete card",
    exported: "Panel JSON exported",
    exportFailed: "Export failed",
    panelTitleLabel: "Panel title",
    panelDescriptionLabel: "Panel description",
    cardTitleLabel: "Card title",
    cardDescriptionLabel: "Card description",
    cardCount: (count: number) => `${count} ${count === 1 ? "card" : "cards"}`,
    emptyTitle: "No BI cards yet",
    emptyDescription:
      "Ask the AI on the left for a table or chart, then confirm the action to import it here.",
    refresh: "Refresh data",
    editCard: "Edit card",
    deleteCard: "Delete card",
    sqlSource: (count: number) => `${count} snapshot ${count === 1 ? "row" : "rows"}`,
    cardKinds: {
      table: "Table",
      chart: "Chart",
    },
    metrics: {
      totalQueries: "Total queries",
      passRate: "Pass rate",
      riskEvents: "Risk events",
      pendingReview: "Pending review",
      highRisk: "High risk",
    },
    weekdays: {
      monday: "Mon",
      tuesday: "Tue",
      wednesday: "Wed",
      thursday: "Thu",
      friday: "Fri",
      saturday: "Sat",
      sunday: "Sun",
    },
    chartKeys: {
      queries: "Queries",
      risks: "Risks",
      passRate: "Pass rate",
      count: "Count",
    },
    categories: {
      customerProfile: "Customer profile",
      orderFacts: "Order facts",
      accessAudit: "Access audit",
      marketing: "Marketing analytics",
      inventory: "Inventory sync",
    },
    owners: {
      dataGovernance: "Data governance",
      transactionPlatform: "Transaction platform",
      securityPlatform: "Security platform",
      growthAnalytics: "Growth analytics",
      supplyChain: "Supply chain",
    },
    updatedAt: {
      tenMinutes: "10 minutes ago",
      eighteenMinutes: "18 minutes ago",
      twentyFourMinutes: "24 minutes ago",
      fortyTwoMinutes: "42 minutes ago",
      oneHour: "1 hour ago",
    },
    riskLevels: {
      low: "Low",
      medium: "Medium",
      high: "High",
      critical: "Critical",
    },
    datasetStatuses: {
      normal: "Normal",
      watch: "Watch",
      needsAction: "Needs action",
    },
    trendTitle: "Query trend",
    trendDescription: "Changes in query volume, risks, and pass rate",
    riskDistributionTitle: "Risk distribution",
    riskDistributionDescription: "Risk events grouped by data domain",
    datasetTableTitle: "Dataset details",
    datasetTableDescription: "Mock data shown by risk priority",
    tableHeaders: {
      dataset: "Dataset",
      owner: "Owner",
      queries: "Queries",
      risk: "Risk level",
      status: "Status",
      updatedAt: "Updated",
    },
  },
};

const dictionaries: Record<Locale, Messages> = {
  "zh-CN": zhCN,
  "en-US": enUS,
};

type I18nContextValue = {
  locale: Locale;
  setLocale: (locale: Locale) => void;
  t: Messages;
};

const I18nContext = createContext<I18nContextValue | null>(null);

export function I18nProvider({ children }: { children: ReactNode }) {
  const [locale, setLocaleState] = useState<Locale>(DEFAULT_LOCALE);

  useEffect(() => {
    const storedLocale = window.localStorage.getItem(LANGUAGE_STORAGE_KEY);

    if (isLocale(storedLocale)) {
      setLocaleState(storedLocale);
      return;
    }

    window.localStorage.setItem(LANGUAGE_STORAGE_KEY, DEFAULT_LOCALE);
  }, []);

  useEffect(() => {
    document.documentElement.lang = locale;
    window.localStorage.setItem(LANGUAGE_STORAGE_KEY, locale);
  }, [locale]);

  const value = useMemo<I18nContextValue>(
    () => ({
      locale,
      setLocale: setLocaleState,
      t: dictionaries[locale],
    }),
    [locale],
  );

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n() {
  const value = useContext(I18nContext);

  if (!value) {
    throw new Error("useI18n must be used within I18nProvider");
  }

  return value;
}

export function getMessages(locale: Locale) {
  return dictionaries[locale];
}

function isLocale(value: string | null): value is Locale {
  return localeOptions.some((locale) => locale.value === value);
}
