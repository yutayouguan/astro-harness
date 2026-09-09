export type HomeStorageState =
  | "new_install"
  | "ready"
  | "layout_migration_required"
  | "settings_migration_required"
  | "migration_incomplete"
  | "invalid_config"
  | "unreadable";
export type StorageDomain = {
  id: string;
  bytes: number;
  files: number;
  skippedLinks: number;
  previewBytes: number;
  previewFiles: number;
};
export type StorageReport = {
  rootPath: string;
  configPath: string;
  state: HomeStorageState;
  settingsVersion: number | null;
  configPresent: boolean;
  partial: boolean;
  inspectedEntries: number;
  domains: StorageDomain[];
  issues: { code: string; path: string }[];
  cleanupPreview: { path: string; bytes: number; policy: string }[];
  previewPartial: boolean;
  cachePolicies: {
    domain: string;
    directory: string;
    enabled: boolean;
    ttlSeconds: number;
    maxSizeMb: number;
    status: string;
  }[];
};

const states: HomeStorageState[] = [
  "new_install",
  "ready",
  "layout_migration_required",
  "settings_migration_required",
  "migration_incomplete",
  "invalid_config",
  "unreadable",
];
const object = (value: unknown): value is Record<string, unknown> =>
  value !== null && typeof value === "object";
const count = (value: unknown): value is number =>
  typeof value === "number" && Number.isFinite(value) && value >= 0;
export function parseStorageReport(value: unknown): StorageReport {
  if (
    !object(value) ||
    typeof value.rootPath !== "string" ||
    typeof value.configPath !== "string" ||
    !states.includes(value.state as HomeStorageState) ||
    typeof value.partial !== "boolean" ||
    typeof value.configPresent !== "boolean" ||
    typeof value.previewPartial !== "boolean" ||
    !Array.isArray(value.cachePolicies) ||
    !value.cachePolicies.every(
      (p) =>
        object(p) &&
        typeof p.domain === "string" &&
        typeof p.directory === "string" &&
        typeof p.enabled === "boolean" &&
        count(p.ttlSeconds) &&
        count(p.maxSizeMb) &&
        typeof p.status === "string",
    ) ||
    !count(value.inspectedEntries) ||
    !(value.settingsVersion === null || count(value.settingsVersion)) ||
    !Array.isArray(value.domains) ||
    !value.domains.every(
      (d) =>
        object(d) &&
        typeof d.id === "string" &&
        [
          d.bytes,
          d.files,
          d.skippedLinks,
          d.previewBytes,
          d.previewFiles,
        ].every(count),
    ) ||
    !Array.isArray(value.issues) ||
    !value.issues.every(
      (i) =>
        object(i) && typeof i.code === "string" && typeof i.path === "string",
    ) ||
    !Array.isArray(value.cleanupPreview) ||
    !value.cleanupPreview.every(
      (i) =>
        object(i) &&
        typeof i.path === "string" &&
        typeof i.policy === "string" &&
        count(i.bytes),
    )
  ) {
    throw new Error("invalid_storage_report");
  }
  return value as StorageReport;
}

export function storageTotals(report: StorageReport) {
  return report.domains.reduce(
    (sum, d) => ({
      bytes: sum.bytes + d.bytes,
      files: sum.files + d.files,
      previewBytes: sum.previewBytes + d.previewBytes,
      previewFiles: sum.previewFiles + d.previewFiles,
      skippedLinks: sum.skippedLinks + d.skippedLinks,
    }),
    { bytes: 0, files: 0, previewBytes: 0, previewFiles: 0, skippedLinks: 0 },
  );
}

export function storageBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  const index = Math.max(
    0,
    Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1),
  );
  return `${new Intl.NumberFormat(undefined, { maximumFractionDigits: index ? 1 : 0 }).format(bytes / 1024 ** index)} ${units[index]}`;
}

export const storageCopy = {
  zh: {
    title: "存储与配置",
    sub: "检查与预览只读 · 移动文件需单独确认",
    refresh: "检查存储",
    loading: "正在检查…",
    error: "无法完成存储检查，请重试。",
    root: "数据根目录",
    config: "全局配置",
    missing: "尚未创建",
    version: "设置版本",
    defaults: "默认设置（尚未显式保存）",
    total: "已扫描占用",
    domains: "按领域查看",
    files: "个文件",
    partial: "已达到扫描上限或存在不可读项，以下大小仅为已扫描部分。",
    links: "个软链接已跳过",
    issues: "检查结果",
    preview: "清理预览",
    candidates: "个候选文件",
    empty: "没有符合当前保留策略的候选文件。",
    sample: "最多展示 30 个样本；预览不代表可立即安全删除。",
    policies: "保留策略",
    enabled: "已启用",
    disabled: "已停用",
    ttl: "TTL（秒）",
    capacity: "容量上限",
    previewPartial: "部分缓存目录或文件未验证，未纳入候选；此预览可能不完整。",
    policyStates: {
      external: "数据根以外的目录未扫描",
      unverified: "目录无法安全验证",
      invalid: "目录策略无效",
      overlap: "缓存目录重叠，未生成候选",
    },
    cache:
      "模型与 MCP 缓存：按 config.toml 的当前目录和 TTL 检查，仅列出已验证归属且过期的 Astro 缓存。停用缓存、未知文件不自动列入。",
    logs: "运行日志：保留至少 30 天；安全审计不在清理范围内。",
    backups: "备份：仅人工审阅，按完整备份集处理，不拆删单个文件。",
    protected:
      "会话、数据库、rollout、工作区、技能、壁纸及浏览器登录数据均受保护。",
    references: "资源检查仅覆盖当前/最近壁纸和活动主题，不代表完整引用分析。",
    states: {
      new_install: "尚未初始化",
      ready: "存储可读取",
      layout_migration_required: "需要目录迁移",
      settings_migration_required: "需要设置迁移",
      migration_incomplete: "上次迁移未完成",
      invalid_config: "配置无效",
      unreadable: "存在不可读配置或目录",
    },
    guidance: {
      new_install: "首次启动时由应用初始化；检查本身不会创建目录。",
      ready:
        "已检查 TOML 语法与桌面设置字段；不代表模型连接或数据库健康检查通过。",
      layout_migration_required:
        "先停止应用并审阅目录迁移清单，参见 docs/home-layout.md。",
      settings_migration_required:
        "旧 JSON 尚未导入 TOML；先预览 astro-migrate-config，参见 docs/global-settings.md。",
      migration_incomplete:
        "请检查备份中的迁移记录，恢复或完成迁移后再启动；不要清空数据根。",
      invalid_config:
        "请检查 config.toml 的语法、字段类型和设置版本；检查不会用默认值覆盖原文件。",
      unreadable: "请检查路径、文件类型和访问权限；软链接不会被自动跟随。",
    },
    domainNames: {
      mcp: "MCP 扩展",
      hooks: "Hooks 定义",
      agents: "Agent",
      artifacts: "文件与知识",
      automation: "自动化",
      backups: "备份",
      browser: "浏览器",
      evolution: "学习与进化",
      logs: "运行日志",
      memory: "记忆",
      models: "模型",
      security: "安全审计",
      sessions: "会话与历史",
      skills: "技能",
      tools: "工具",
      ui: "界面资源",
      usage: "用量",
      workspace: "工作区",
      other: "根配置及其他文件",
    },
    issueNames: {
      cache_policy_invalid: "缓存策略无效",
      cache_external_not_scanned: "外部缓存未计入占用和预览",
      cache_directory_unverified: "缓存目录未安全验证",
      cache_directories_overlap: "缓存目录重叠，预览已跳过",
      cache_entry_unverified: "缓存封装无效或超过读取预算，已跳过",
      root_unreadable: "数据根不可读",
      migration_incomplete: "迁移未完成",
      layout_migration_required: "发现旧目录",
      settings_migration_required: "发现未导入的 JSON 设置",
      config_invalid: "配置语法、字段或版本无效",
      config_unreadable: "配置不可读、过大或为软链接",
      scan_unreadable: "部分目录无法读取",
      resource_missing: "资源引用已失效",
      resource_skipped: "资源路径未检查",
      resource_outside_root: "数据根以外的资源未检查",
      resource_manifest_invalid: "活动主题清单无效",
    },
  },
  en: {
    title: "Storage & configuration",
    sub: "Read-only inspection · Moving files requires separate confirmation",
    refresh: "Inspect storage",
    loading: "Inspecting…",
    error: "Storage inspection failed. Please retry.",
    root: "Data root",
    config: "Global configuration",
    missing: "Not created",
    version: "Settings version",
    defaults: "Defaults (not explicitly saved)",
    total: "Scanned size",
    domains: "Browse by domain",
    files: "files",
    partial:
      "A scan limit or unreadable entry was encountered. Sizes are lower bounds.",
    links: "symlinks skipped",
    issues: "Findings",
    preview: "Cleanup preview",
    candidates: "candidate files",
    empty: "No files meet the current retention policies.",
    sample:
      "Up to 30 samples. A candidate is not a guarantee of safe deletion.",
    policies: "Retention policies",
    enabled: "Enabled",
    disabled: "Disabled",
    ttl: "TTL (seconds)",
    capacity: "Capacity limit",
    previewPartial:
      "Some cache directories or files could not be verified and were excluded. This preview may be incomplete.",
    policyStates: {
      external: "Directory outside the data root was not scanned",
      unverified: "Directory could not be safely verified",
      invalid: "Invalid directory policy",
      overlap: "Overlapping cache directories excluded",
    },
    cache:
      "Model and MCP caches: use the configured directory and TTL. Only verified, expired Astro entries are candidates. Disabled caches and unknown files are excluded.",
    logs: "Runtime logs: keep at least 30 days. Security audits are excluded.",
    backups: "Backups: manual review only. Treat each backup set as a unit.",
    protected:
      "Sessions, databases, rollouts, workspace, skills, wallpapers and browser login data are protected.",
    references:
      "Reference checks cover current/recent wallpapers and the active theme, not every resource reference.",
    states: {
      new_install: "Not initialized",
      ready: "Storage readable",
      layout_migration_required: "Layout migration required",
      settings_migration_required: "Settings migration required",
      migration_incomplete: "Migration incomplete",
      invalid_config: "Invalid configuration",
      unreadable: "Unreadable configuration or directory",
    },
    guidance: {
      new_install:
        "The app initializes a new home on startup. Inspection creates nothing.",
      ready:
        "TOML syntax and Desktop setting fields checked. This is not a model connectivity or database health check.",
      layout_migration_required:
        "Stop the app and review the layout migration plan. See docs/home-layout.md.",
      settings_migration_required:
        "Legacy JSON has not been imported. Preview astro-migrate-config; see docs/global-settings.md.",
      migration_incomplete:
        "Review the backup migration record before restarting. Do not clear the data root.",
      invalid_config:
        "Check config.toml syntax, field types and version. Inspection will not overwrite it with defaults.",
      unreadable:
        "Check paths, file types and permissions. Symlinks are not followed automatically.",
    },
    domainNames: {
      mcp: "MCP extensions",
      hooks: "Hook definitions",
      agents: "Agents",
      artifacts: "Files & knowledge",
      automation: "Automation",
      backups: "Backups",
      browser: "Browser",
      evolution: "Learning & evolution",
      logs: "Runtime logs",
      memory: "Memory",
      models: "Models",
      security: "Security audits",
      sessions: "Sessions & history",
      skills: "Skills",
      tools: "Tools",
      ui: "UI assets",
      usage: "Usage",
      workspace: "Workspace",
      other: "Root settings & other files",
    },
    issueNames: {
      cache_policy_invalid: "Invalid cache policy",
      cache_external_not_scanned:
        "External cache excluded from totals and preview",
      cache_directory_unverified: "Unverified cache directory",
      cache_directories_overlap: "Overlapping cache directories excluded",
      cache_entry_unverified: "Invalid or over-budget cache envelope excluded",
      root_unreadable: "Unreadable data root",
      migration_incomplete: "Incomplete migration",
      layout_migration_required: "Retired layout found",
      settings_migration_required: "Unimported JSON settings found",
      config_invalid: "Invalid syntax, fields or version",
      config_unreadable: "Configuration unreadable, oversized or symlinked",
      scan_unreadable: "Directory could not be read",
      resource_missing: "Missing referenced resource",
      resource_skipped: "Resource path not checked",
      resource_outside_root: "Resource outside data root not checked",
      resource_manifest_invalid: "Invalid active theme manifest",
    },
  },
} as const;
