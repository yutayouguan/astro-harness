/** 偏好设置（主题、语言、日志诊断、关于）。 */
import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import type { LucideIcon } from "lucide-react";
import {
  Activity,
  ArrowUp,
  Brain,
  ChevronsUpDown,
  Clock,
  Copyright,
  Dices,
  Download,
  Palette,
  Play,
  Plug,
  RefreshCw,
  Search,
  ScrollText,
  Sparkles,
  Webhook,
  Layers,
  List,
  Wrench,
} from "lucide-react";
import { Activity as ActivityData, Sparkles as SparklesData } from "lucide";
import {
  getIdentifier,
  getTauriVersion,
  getVersion,
} from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { motion, useReducedMotion } from "framer-motion";
import appIconAsset from "../../assets/astro-app-icon.png";
import { useAppIcon } from "../../hooks/settings/useAppIcon";
import { useMorphicons } from "../../hooks/app/useMorphicons";
import type { WallpaperController } from "../../hooks/app/useWallpaper";
import type { AppIconId } from "../../types";
import type { ShellColorStyle } from "../../hooks/app/useShellColorStyle";
import {
  useTheme,
  type ThemeMode,
  type GlassLevel,
} from "../../hooks/app/useTheme";
import type {
  ChatAnswerLayout,
  ChatDisplayPrefs,
  ChatDisplayToggleKey,
  ChatVerbosity,
} from "../../hooks/chat/useChatDisplayPrefs";
import { useI18n } from "../../i18n/LocaleContext";
import type { Locale, MessageKey } from "../../i18n/messages";
import {
  gradientFromPreset,
  gradientSwatchBackground,
  SHELL_GRADIENT_PRESETS,
  type ShellGradient,
} from "../../lib/ui/shellGradient";
import {
  MORPHICON_SPRINGS,
  MORPHICON_STROKE_WIDTHS,
  type MorphiconSpring,
  type MorphiconStrokeWidth,
} from "../../lib/ui/morphiconPrefs";
import { AppMorphIcon } from "../icons/MorphIcon";
import { IconGlobe, IconChat, IconAtom } from "../icons/NavIcons";
import { SelectMenu } from "../ui/SelectMenu";
import CompressionSettingsCard from "./CompressionSettingsCard";
import ShellGradientEditor from "./ShellGradientEditor";
import WallpaperSettingsCard from "./WallpaperSettingsCard";

/** 查询返回的单行日志 */
type AgentLogLine = { raw: string; source: string };

type DiagnosticsStatusDto = {
  backendHealthy: boolean;
  backendEndpoint: string;
  backendError: string | null;
  providerEnabled: number;
  providerTotal: number;
  activeProviderId: string | null;
  providerError: string | null;
  mcpConnected: number;
  mcpTotal: number;
  mcpRetrying: number;
  mcpError: string | null;
  databaseHealthy: boolean;
  databaseJournalMode: string;
  databaseSchemaVersion: number | null;
  databaseError: string | null;
};

/** 日志来源过滤 */
type LogSourceFilter = "both" | "agent" | "errors";

/** 查询范围：本次会话 / 全部会话 */
type LogScope = "current" | "all";

/** 内容过滤：全部 / 只看问题（warn 及以上） */
type LogLevelFilter = "all" | "issues";

type DiagnosticLogLevel = "error" | "warn" | "info" | "debug" | "unknown";

type DiagnosticStatusCardModel = {
  id: string;
  label: string;
  value: string;
  detail: string;
  state: "healthy" | "warning" | "error" | "unknown";
};

function diagnosticLogLevel(raw: string): DiagnosticLogLevel {
  const upper = raw.toUpperCase();
  if (upper.includes("CRITICAL") || upper.includes("ERROR")) return "error";
  if (upper.includes("WARNING") || upper.includes("WARN")) return "warn";
  if (upper.includes("INFO")) return "info";
  if (upper.includes("DEBUG") || upper.includes("TRACE")) return "debug";
  return "unknown";
}

function DiagnosticStatusCard({
  label,
  value,
  detail,
  state,
}: {
  label: string;
  value: string;
  detail: string;
  state: "healthy" | "warning" | "error" | "unknown";
}) {
  return (
    <div className="prefs-diag-status-card" data-status={state}>
      <span>{label}</span>
      <strong>{value}</strong>
      <small>{detail}</small>
    </div>
  );
}

type AppUpdateInfo = {
  configured: boolean;
  available: boolean;
  currentVersion: string;
  version: string | null;
  date: string | null;
  notes: string | null;
};

type AppUpdatePhase =
  | "idle"
  | "checking"
  | "unconfigured"
  | "current"
  | "available"
  | "installing"
  | "error";

type AppUpdateProgress = {
  phase: "downloading" | "installing";
  downloaded: number;
  total: number | null;
};

/** 弹簧预设的本地化文案 */
const MORPHICON_SPRING_LABEL: Record<MorphiconSpring, MessageKey> = {
  smooth: "prefs.morphicons.spring.smooth",
  snappy: "prefs.morphicons.spring.snappy",
  bouncy: "prefs.morphicons.spring.bouncy",
};

const MORPHICON_STROKE_LABEL: Record<MorphiconStrokeWidth, MessageKey> = {
  1: "prefs.appearance.motion.stroke.thin",
  2: "prefs.appearance.motion.stroke.regular",
  2.5: "prefs.appearance.motion.stroke.bold",
};

const ABOUT_COPY = {
  zh: {
    development: "开发构建",
    updates: "检查更新",
    updatesPrompt: "从公开发布仓库检查已签名安装包",
    checking: "正在检查…",
    current: "当前已是最新版本",
    available: "发现新版本 {{version}}",
    install: "下载并安装",
    installing: "正在下载并安装…",
    progress: "已下载 {{progress}}%",
    retry: "重试",
    updatesUnavailable: "此构建未配置自动更新服务",
    license: "许可证",
    licenseValue: "私有项目，未声明开源许可",
    platform: "Universal",
    stable: "稳定版",
    updateTitle: "更新",
    updateSub: "当前使用稳定更新通道。",
    projectTitle: "项目信息",
    projectSub: "Astro 及其配置与数据均保存在本机。",
    view: "查看",
    collapse: "收起",
    dataDirectory: "数据目录",
  },
  en: {
    development: "Development build",
    updates: "Check for updates",
    updatesPrompt: "Check the public release repository for a signed build",
    checking: "Checking…",
    current: "You are up to date",
    available: "Version {{version}} is available",
    install: "Download and install",
    installing: "Downloading and installing…",
    progress: "Downloaded {{progress}}%",
    retry: "Retry",
    updatesUnavailable: "Automatic updates are not configured for this build",
    license: "License",
    licenseValue: "Private project; no open-source license declared",
    platform: "Universal",
    stable: "Stable",
    updateTitle: "Updates",
    updateSub: "You are using the stable update channel.",
    projectTitle: "Project information",
    projectSub: "Astro, its configuration, and data stay on this device.",
    view: "View",
    collapse: "Hide",
    dataDirectory: "Data directory",
  },
} as const;

function AutostartSwitch({ tone }: { tone: string }) {
  const [enabled, setEnabled] = useState(false);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    import("@tauri-apps/plugin-autostart")
      .then((mod) => {
        mod
          .isEnabled()
          .then((v) => {
            setEnabled(v);
            setLoading(false);
          })
          .catch(() => setLoading(false));
      })
      .catch(() => setLoading(false));
  }, []);

  const toggle = async () => {
    try {
      const mod = await import("@tauri-apps/plugin-autostart");
      if (enabled) {
        await mod.disable();
      } else {
        await mod.enable();
      }
      setEnabled(!enabled);
    } catch (e) {
      console.error("autostart toggle failed", e);
    }
  };

  if (loading) return null;

  return (
    <button
      type="button"
      role="switch"
      className="prefs-switch"
      aria-checked={enabled}
      data-tone={tone}
      onClick={toggle}
    >
      <span className="prefs-switch-thumb" />
    </button>
  );
}

/** 行数预设 */
const LINE_PRESETS = [50, 100, 200, 500] as const;

export type PreferenceCategory =
  | "general"
  | "appearance"
  | "conversation"
  | "context"
  | "diagnostics"
  | "about";

type Props = {
  mode: ThemeMode;
  onChange: (mode: ThemeMode) => void;
  colorStyle: ShellColorStyle;
  onColorStyleChange: (style: ShellColorStyle) => void;
  gradient: ShellGradient;
  onGradientChange: (gradient: ShellGradient) => void;
  onBeginCustomGradient: () => void;
  onPreviewGradient: (gradient: ShellGradient) => void;
  onCommitCustomGradient: (gradient?: ShellGradient) => void;
  onCancelCustomGradient: () => void;
  onReshuffleDynamic: () => void;
  wallpaper: WallpaperController;
  tone?: string;
  chatDisplayPrefs: ChatDisplayPrefs;
  onChatVerbosityChange: (verbosity: ChatVerbosity) => void;
  onChatAnswerLayoutChange: (layout: ChatAnswerLayout) => void;
  onChatToggleChange: (key: ChatDisplayToggleKey, value: boolean) => void;
  activeSessionId?: string;
  /** 由外部 settings 侧栏控制显示哪个分类；未传则显示内部导航 */
  section?: PreferenceCategory;
};

/** 聊天展示开关字段（不含 verbosity） */
type ToggleKey = ChatDisplayToggleKey;

const TOGGLE_KEYS: {
  key: ToggleKey;
  labelKey: MessageKey;
  descKey: MessageKey;
  Icon: LucideIcon;
}[] = [
  {
    key: "processDefaultOpen",
    labelKey: "prefs.chat.processDefaultOpen",
    descKey: "prefs.chat.processDefaultOpenDesc",
    Icon: ChevronsUpDown,
  },
  {
    key: "showTools",
    labelKey: "prefs.chat.showTools",
    descKey: "prefs.chat.showToolsDesc",
    Icon: Wrench,
  },
  {
    key: "showSkills",
    labelKey: "prefs.chat.showSkills",
    descKey: "prefs.chat.showSkillsDesc",
    Icon: Sparkles,
  },
  {
    key: "showMcp",
    labelKey: "prefs.chat.showMcp",
    descKey: "prefs.chat.showMcpDesc",
    Icon: Plug,
  },
  {
    key: "showHooks",
    labelKey: "prefs.chat.showHooks",
    descKey: "prefs.chat.showHooksDesc",
    Icon: Webhook,
  },
  {
    key: "showMemory",
    labelKey: "prefs.chat.showMemory",
    descKey: "prefs.chat.showMemoryDesc",
    Icon: Brain,
  },
  {
    key: "showStatus",
    labelKey: "prefs.chat.showStatus",
    descKey: "prefs.chat.showStatusDesc",
    Icon: Activity,
  },
  {
    key: "showTimestamps",
    labelKey: "prefs.chat.showTimestamps",
    descKey: "prefs.chat.showTimestampsDesc",
    Icon: Clock,
  },
];

function ConversationLayoutPreview({ prefs }: { prefs: ChatDisplayPrefs }) {
  const { t } = useI18n();
  const activities: {
    key: ChatDisplayToggleKey;
    label: MessageKey;
    Icon: LucideIcon;
  }[] = [
    { key: "showStatus", label: "prefs.chat.preview.status", Icon: Activity },
    { key: "showTools", label: "prefs.chat.preview.tool", Icon: Wrench },
    { key: "showSkills", label: "prefs.chat.preview.skill", Icon: Sparkles },
    { key: "showMcp", label: "prefs.chat.preview.mcp", Icon: Plug },
    { key: "showHooks", label: "prefs.chat.preview.hook", Icon: Webhook },
    { key: "showMemory", label: "prefs.chat.preview.memory", Icon: Brain },
  ];
  const visibleActivities = activities.filter(({ key }) => prefs[key]);
  const shownActivities = visibleActivities.slice(0, 4);
  const hiddenCount = visibleActivities.length - shownActivities.length;
  const answerKey =
    `prefs.chat.preview.answer.${prefs.verbosity}` as MessageKey;

  return (
    <div
      className="prefs-conversation-preview"
      data-layout={prefs.answerLayout}
      aria-label={t("prefs.chat.preview.title")}
    >
      <div className="prefs-conversation-preview-head">
        <span>
          <i aria-hidden />
          {t("prefs.chat.preview.title")}
        </span>
        <span>
          {t(`prefs.chat.layout.${prefs.answerLayout}` as MessageKey)}
        </span>
      </div>
      <div className="prefs-conversation-preview-feed">
        <div className="prefs-conversation-preview-user">
          <span>{t("prefs.chat.preview.user")}</span>
          {prefs.showTimestamps ? <time>10:42</time> : null}
        </div>
        <div className="prefs-conversation-preview-assistant">
          <span className="prefs-conversation-preview-avatar" aria-hidden>
            A
          </span>
          <div className="prefs-conversation-preview-turn">
            {shownActivities.length > 0 ? (
              prefs.answerLayout === "grouped" ? (
                <div className="prefs-conversation-preview-group">
                  <span className="prefs-conversation-preview-icons">
                    {shownActivities.map(({ key, Icon }) => (
                      <i key={key}>
                        <Icon size={11} strokeWidth={2.2} />
                      </i>
                    ))}
                  </span>
                  <span>
                    {t("prefs.chat.preview.grouped", {
                      count: String(visibleActivities.length),
                    })}
                  </span>
                  <b aria-hidden>›</b>
                </div>
              ) : (
                <div className="prefs-conversation-preview-activities">
                  {shownActivities.map(({ key, label, Icon }) => (
                    <div key={key}>
                      <i aria-hidden>
                        <Icon size={11} strokeWidth={2.2} />
                      </i>
                      <span>{t(label)}</span>
                      <small>{t("prefs.chat.preview.done")}</small>
                    </div>
                  ))}
                  {hiddenCount > 0 ? (
                    <div className="prefs-conversation-preview-more">
                      +{hiddenCount}
                    </div>
                  ) : null}
                </div>
              )
            ) : null}
            <div className="prefs-conversation-preview-answer">
              <strong>Astro</strong>
              <p>{t(answerKey)}</p>
              {prefs.showTimestamps ? <time>10:42</time> : null}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

export default function PreferencesPanel({
  mode,
  onChange,
  colorStyle,
  onColorStyleChange,
  gradient,
  onGradientChange,
  onBeginCustomGradient,
  onPreviewGradient,
  onCommitCustomGradient,
  onCancelCustomGradient,
  onReshuffleDynamic,
  wallpaper,
  tone = "twilight",
  chatDisplayPrefs: prefs,
  onChatVerbosityChange,
  onChatAnswerLayoutChange,
  onChatToggleChange,
  activeSessionId,
  section,
}: Props) {
  const { locale, setLocale, t } = useI18n();
  const { glassLevel, setGlassLevel } = useTheme();
  const { spring, strokeWidth, setSpring, setStrokeWidth } = useMorphicons();
  const reduceMotion = useReducedMotion();
  const { settings: appIcon, setIcon: setAppIcon } = useAppIcon();
  const [gradientEditorOpen, setGradientEditorOpen] = useState(false);
  const [appMeta, setAppMeta] = useState({
    version: "0.1.0",
    identifier: "com.astroagent.desktop",
    runtime: "Tauri 2",
  });
  const [updateInfo, setUpdateInfo] = useState<AppUpdateInfo | null>(null);
  const [updatePhase, setUpdatePhase] = useState<AppUpdatePhase>("idle");
  const [updateProgress, setUpdateProgress] = useState<number | null>(null);
  const [updateError, setUpdateError] = useState("");
  const [licenseExpanded, setLicenseExpanded] = useState(false);
  const [morphPreviewActive, setMorphPreviewActive] = useState(false);
  // 预览靠翻转图标来触发一次形变；任何预设改动都在同一批渲染里带上新参数重播。
  const playMorphPreview = useCallback(() => {
    setMorphPreviewActive((value) => !value);
  }, []);
  const [internalCategory, setInternalCategory] =
    useState<PreferenceCategory>("general");
  const activeCategory = section ?? internalCategory;
  const setActiveCategory = section ? () => {} : setInternalCategory;
  const showInternalNav = !section;

  useEffect(() => {
    if (activeCategory !== "about") return;
    let cancelled = false;
    void Promise.all([getVersion(), getIdentifier(), getTauriVersion()])
      .then(([version, identifier, tauriVersion]) => {
        if (!cancelled) {
          setAppMeta({
            version: version || "0.1.0",
            identifier: identifier || "com.astroagent.desktop",
            runtime: `Tauri ${tauriVersion || "2"}`,
          });
        }
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [activeCategory]);

  useEffect(() => {
    if (activeCategory !== "about") return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<AppUpdateProgress>("app-update-progress", (event) => {
      if (disposed) return;
      const { downloaded, total } = event.payload;
      setUpdatePhase("installing");
      setUpdateProgress(
        total && total > 0
          ? Math.min(100, Math.round((downloaded / total) * 100))
          : null,
      );
    })
      .then((cleanup) => {
        if (disposed) cleanup();
        else unlisten = cleanup;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [activeCategory]);

  const checkForUpdate = useCallback(async () => {
    setUpdatePhase("checking");
    setUpdateError("");
    setUpdateProgress(null);
    try {
      const info = await invoke<AppUpdateInfo>("check_app_update");
      setUpdateInfo(info);
      setUpdatePhase(
        !info.configured
          ? "unconfigured"
          : info.available
            ? "available"
            : "current",
      );
    } catch (error) {
      setUpdatePhase("error");
      setUpdateError(error instanceof Error ? error.message : String(error));
    }
  }, []);

  const installUpdate = useCallback(async () => {
    if (!updateInfo?.available) return;
    setUpdatePhase("installing");
    setUpdateProgress(null);
    setUpdateError("");
    try {
      await invoke("install_app_update");
    } catch (error) {
      setUpdatePhase("error");
      setUpdateError(error instanceof Error ? error.message : String(error));
    }
  }, [updateInfo]);

  const appIconLabel = (id: AppIconId): string => {
    switch (id) {
      case "blue":
        return t("prefs.appIcon.blue");
      case "deep_blue":
        return t("prefs.appIcon.deepBlue");
      case "black":
        return t("prefs.appIcon.black");
      case "white":
        return t("prefs.appIcon.white");
      case "white_logo":
        return t("prefs.appIcon.whiteLogo");
      default:
        return id;
    }
  };

  const hasSession = Boolean(activeSessionId);
  const [scope, setScope] = useState<LogScope>(hasSession ? "current" : "all");
  const [level, setLevel] = useState<LogLevelFilter>("all");
  const [lines, setLines] = useState<number>(50);
  const [source, setSource] = useState<LogSourceFilter>("both");
  const [manualSession, setManualSession] = useState("");
  const [turnId, setTurnId] = useState("");
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [logSearch, setLogSearch] = useState("");
  const [logsCopied, setLogsCopied] = useState(false);
  const [rows, setRows] = useState<AgentLogLine[]>([]);
  const [diagnosticsStatus, setDiagnosticsStatus] =
    useState<DiagnosticsStatusDto | null>(null);
  const [diagnosticsStatusError, setDiagnosticsStatusError] = useState("");
  const [diagnosticsStatusBusy, setDiagnosticsStatusBusy] = useState(false);
  const [exportingDiagnostics, setExportingDiagnostics] = useState(false);
  const [diagnosticsExportPath, setDiagnosticsExportPath] = useState("");
  const [busy, setBusy] = useState(false);
  const [errorMsg, setErrorMsg] = useState("");
  const [queried, setQueried] = useState(false);
  const normalizedLogSearch = logSearch.trim().toLowerCase();
  const visibleLogRows = normalizedLogSearch
    ? rows.filter((row) =>
        `${row.source} ${row.raw}`.toLowerCase().includes(normalizedLogSearch),
      )
    : rows;
  const diagnosticCards: DiagnosticStatusCardModel[] = diagnosticsStatus
    ? [
        {
          id: "backend",
          label: t("prefs.diag.status.backend"),
          value: t(
            diagnosticsStatus.backendHealthy
              ? "prefs.diag.status.healthy"
              : "prefs.diag.status.unavailable",
          ),
          detail: diagnosticsStatus.backendHealthy
            ? t("prefs.diag.status.backendDetail", {
                endpoint: diagnosticsStatus.backendEndpoint.replace(
                  /^https?:\/\//,
                  "",
                ),
              })
            : t("prefs.diag.status.unavailable"),
          state: diagnosticsStatus.backendHealthy ? "healthy" : "error",
        },
        {
          id: "provider",
          label: t("prefs.diag.status.provider"),
          value: `${diagnosticsStatus.providerEnabled}/${diagnosticsStatus.providerTotal}`,
          detail: diagnosticsStatus.providerError
            ? t("prefs.diag.status.unavailable")
            : diagnosticsStatus.activeProviderId
              ? t("prefs.diag.status.providerDetail", {
                  provider: diagnosticsStatus.activeProviderId,
                })
              : t("prefs.diag.status.noneActive"),
          state: diagnosticsStatus.providerError
            ? "error"
            : diagnosticsStatus.providerEnabled ===
                  diagnosticsStatus.providerTotal &&
                diagnosticsStatus.providerTotal > 0
              ? "healthy"
              : "warning",
        },
        {
          id: "mcp",
          label: t("prefs.diag.status.mcp"),
          value: `${diagnosticsStatus.mcpConnected}/${diagnosticsStatus.mcpTotal}`,
          detail: diagnosticsStatus.mcpError
            ? t("prefs.diag.status.unavailable")
            : diagnosticsStatus.mcpTotal === 0
              ? t("prefs.diag.status.mcpNone")
              : diagnosticsStatus.mcpRetrying > 0
                ? t("prefs.diag.status.mcpRetrying", {
                    count: String(diagnosticsStatus.mcpRetrying),
                  })
                : diagnosticsStatus.mcpConnected === diagnosticsStatus.mcpTotal
                  ? t("prefs.diag.status.mcpReady")
                  : t("prefs.diag.status.mcpDisconnected", {
                      count: String(
                        diagnosticsStatus.mcpTotal -
                          diagnosticsStatus.mcpConnected,
                      ),
                    }),
          state: diagnosticsStatus.mcpError
            ? "error"
            : diagnosticsStatus.mcpTotal === 0
              ? "unknown"
              : diagnosticsStatus.mcpRetrying > 0 ||
                  diagnosticsStatus.mcpConnected < diagnosticsStatus.mcpTotal
                ? "warning"
                : "healthy",
        },
        {
          id: "database",
          label: t("prefs.diag.status.database"),
          value: diagnosticsStatus.databaseJournalMode,
          detail:
            diagnosticsStatus.databaseHealthy &&
            diagnosticsStatus.databaseSchemaVersion != null
              ? t("prefs.diag.status.databaseDetail", {
                  version: String(diagnosticsStatus.databaseSchemaVersion),
                })
              : t("prefs.diag.status.unavailable"),
          state: diagnosticsStatus.databaseHealthy ? "healthy" : "error",
        },
      ]
    : (["backend", "provider", "mcp", "database"] as const).map((id) => ({
        id,
        label: t(`prefs.diag.status.${id}` as MessageKey),
        value: diagnosticsStatusBusy ? t("prefs.diag.status.checking") : "—",
        detail: diagnosticsStatusError
          ? t("prefs.diag.status.unavailable")
          : t("prefs.diag.status.waiting"),
        state: "unknown" as const,
      }));

  const themeOptions: {
    id: ThemeMode;
    label: string;
  }[] = [
    {
      id: "light",
      label: t("prefs.appearance.theme.light"),
    },
    {
      id: "auto",
      label: t("prefs.appearance.theme.system"),
    },
    {
      id: "dark",
      label: t("prefs.appearance.theme.dark"),
    },
  ];

  const colorStyleOptions: {
    id: ShellColorStyle;
    label: string;
  }[] = [
    {
      id: "unified",
      label: t("prefs.colorStyle.unified"),
    },
    {
      id: "dynamic",
      label: t("prefs.colorStyle.dynamic"),
    },
    {
      id: "colorful",
      label: t("prefs.colorStyle.colorful"),
    },
  ];

  const glassOptions: { id: GlassLevel; label: string }[] = [
    {
      id: "minimal",
      label: t("prefs.appearance.glass.minimal"),
    },
    {
      id: "normal",
      label: t("prefs.appearance.glass.normal"),
    },
    {
      id: "rich",
      label: t("prefs.appearance.glass.rich"),
    },
    {
      id: "liquid-soft",
      label: t("prefs.appearance.glass.liquidSoft"),
    },
    {
      id: "liquid",
      label: t("prefs.appearance.glass.liquid"),
    },
  ];
  const glassLevelIndex = Math.max(
    0,
    glassOptions.findIndex(({ id }) => id === glassLevel),
  );
  const glassLevelProgress =
    glassOptions.length > 1
      ? (glassLevelIndex / (glassOptions.length - 1)) * 100
      : 0;

  const langOptions: {
    id: Locale;
    label: string;
    desc: string;
  }[] = [
    {
      id: "zh",
      label: t("prefs.lang.zh"),
      desc: t("prefs.lang.zhDesc"),
    },
    {
      id: "en",
      label: t("prefs.lang.en"),
      desc: t("prefs.lang.enDesc"),
    },
  ];

  const verbosityOptions: {
    id: ChatVerbosity;
    labelKey: MessageKey;
    descKey: MessageKey;
  }[] = [
    {
      id: "compact",
      labelKey: "prefs.chat.compact",
      descKey: "prefs.chat.compactDesc",
    },
    {
      id: "normal",
      labelKey: "prefs.chat.normal",
      descKey: "prefs.chat.normalDesc",
    },
    {
      id: "detailed",
      labelKey: "prefs.chat.detailed",
      descKey: "prefs.chat.detailedDesc",
    },
  ];

  const answerLayoutOptions: {
    id: ChatAnswerLayout;
    labelKey: MessageKey;
    descKey: MessageKey;
    Icon: LucideIcon;
  }[] = [
    {
      id: "timeline",
      labelKey: "prefs.chat.layout.timeline",
      descKey: "prefs.chat.layout.timelineDesc",
      Icon: List,
    },
    {
      id: "grouped",
      labelKey: "prefs.chat.layout.grouped",
      descKey: "prefs.chat.layout.groupedDesc",
      Icon: Layers,
    },
  ];

  const selectedAppIcon = appIcon?.options.find(
    (option) => option.id === appIcon.current,
  );
  const aboutCopy = ABOUT_COPY[locale];
  const updateStatus =
    updatePhase === "checking"
      ? aboutCopy.checking
      : updatePhase === "current"
        ? aboutCopy.current
        : updatePhase === "available"
          ? aboutCopy.available.replace(
              "{{version}}",
              updateInfo?.version ?? "",
            )
          : updatePhase === "installing"
            ? updateProgress == null
              ? aboutCopy.installing
              : aboutCopy.progress.replace(
                  "{{progress}}",
                  String(updateProgress),
                )
            : updatePhase === "unconfigured"
              ? aboutCopy.updatesUnavailable
              : updatePhase === "error"
                ? updateError
                : aboutCopy.updatesPrompt;
  const categoryOptions = [
    {
      id: "general" as const,
      label: t("prefs.category.general"),
      Icon: IconGlobe,
    },
    {
      id: "appearance" as const,
      label: t("prefs.category.appearance"),
      Icon: Palette,
    },
    {
      id: "conversation" as const,
      label: t("prefs.category.conversation"),
      Icon: IconChat,
    },
    {
      id: "context" as const,
      label: t("prefs.category.context"),
      Icon: Layers,
    },
    {
      id: "diagnostics" as const,
      label: t("prefs.category.diagnostics"),
      Icon: ScrollText,
    },
    {
      id: "about" as const,
      label: t("prefs.category.about"),
      Icon: IconAtom,
    },
  ];

  const refreshRef = useRef<() => Promise<void>>(async () => {});

  async function refreshLogs() {
    const manual = manualSession.trim();
    const effectiveSession =
      manual || (scope === "current" ? (activeSessionId ?? null) : null);
    setBusy(true);
    setErrorMsg("");
    try {
      const result = await invoke<AgentLogLine[]>("query_agent_logs", {
        args: {
          sessionId: effectiveSession || null,
          turnId: turnId.trim() || null,
          source,
          lines,
          minLevel: level === "issues" ? "WARN" : null,
        },
      });
      setRows(result);
      setQueried(true);
    } catch (error) {
      setRows([]);
      setQueried(true);
      setErrorMsg(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  }
  refreshRef.current = refreshLogs;

  const refreshDiagnosticsStatus = useCallback(async () => {
    setDiagnosticsStatusBusy(true);
    setDiagnosticsStatusError("");
    try {
      setDiagnosticsStatus(
        await invoke<DiagnosticsStatusDto>("get_diagnostics_status"),
      );
    } catch (error) {
      setDiagnosticsStatus(null);
      setDiagnosticsStatusError(
        error instanceof Error ? error.message : String(error),
      );
    } finally {
      setDiagnosticsStatusBusy(false);
    }
  }, []);

  useEffect(() => {
    if (activeCategory !== "diagnostics") return;
    const timer = setTimeout(() => {
      void refreshRef.current();
    }, 250);
    return () => clearTimeout(timer);
  }, [activeCategory, scope, level, lines, source, manualSession, turnId]);

  useEffect(() => {
    if (activeCategory === "diagnostics") void refreshDiagnosticsStatus();
  }, [activeCategory, refreshDiagnosticsStatus]);

  useEffect(() => {
    if (!logsCopied) return;
    const timer = window.setTimeout(() => setLogsCopied(false), 1800);
    return () => window.clearTimeout(timer);
  }, [logsCopied]);

  async function copyLogs() {
    const text = visibleLogRows.map((r) => `[${r.source}] ${r.raw}`).join("\n");
    try {
      await navigator.clipboard.writeText(text);
      setLogsCopied(true);
    } catch (e) {
      setErrorMsg(e instanceof Error ? e.message : String(e));
    }
  }

  async function exportDiagnostics() {
    if (exportingDiagnostics) return;
    setExportingDiagnostics(true);
    setErrorMsg("");
    try {
      const path = await invoke<string | null>("export_diagnostics_bundle");
      if (path) setDiagnosticsExportPath(path);
    } catch (error) {
      setErrorMsg(error instanceof Error ? error.message : String(error));
    } finally {
      setExportingDiagnostics(false);
    }
  }

  return (
    <div
      className={`prefs-page ${section ? "is-embedded" : ""}`}
      data-tone={tone}
    >
      <nav
        className="prefs-category-nav"
        aria-label={t("prefs.category.aria")}
        hidden={!showInternalNav}
      >
        {categoryOptions.map(({ id, label, Icon }) => (
          <button
            key={id}
            type="button"
            className={`prefs-category-nav-item ${
              activeCategory === id ? "is-active" : ""
            } ${id === "diagnostics" ? "is-separated" : ""}`}
            aria-current={activeCategory === id ? "page" : undefined}
            onClick={() => setActiveCategory(id)}
          >
            <span className="prefs-category-nav-icon" aria-hidden>
              <Icon width={17} height={17} />
            </span>
            <span>{label}</span>
          </button>
        ))}
      </nav>

      <div className="prefs-category-content">
        <div
          className="prefs-category-stack prefs-category-stack--appearance"
          hidden={activeCategory !== "appearance"}
        >
          <section className="prefs-card prefs-card--appearance-material">
            <div className="prefs-card-head">
              <div className="prefs-icon-badge" data-tone={tone} aria-hidden>
                <span className="appearance-card-symbol appearance-card-symbol--material" />
              </div>
              <div>
                <h2 className="prefs-card-title">
                  {t("prefs.appearance.material.title")}
                </h2>
                <p className="prefs-card-sub">
                  {t("prefs.appearance.material.sub")}
                </p>
              </div>
            </div>

            <div className="appearance-control-list">
              <div className="appearance-control-row appearance-control-row--split">
                <div className="appearance-control-copy">
                  <strong>{t("prefs.appearance.theme.title")}</strong>
                  <span>{t("prefs.appearance.theme.sub")}</span>
                </div>
                <div
                  className="appearance-segmented"
                  role="radiogroup"
                  aria-label={t("prefs.appearance.theme.title")}
                >
                  {themeOptions.map(({ id, label }) => (
                    <button
                      key={id}
                      type="button"
                      role="radio"
                      aria-checked={mode === id}
                      className={mode === id ? "is-active" : ""}
                      onClick={() => onChange(id)}
                    >
                      {label}
                    </button>
                  ))}
                </div>
              </div>

              <div className="appearance-control-row appearance-control-row--split">
                <div className="appearance-control-copy">
                  <strong>{t("prefs.appearance.glass.title")}</strong>
                  <span id="appearance-glass-description">
                    {t("prefs.appearance.glass.sub")}
                  </span>
                </div>
                <div
                  className="appearance-glass-slider"
                  style={
                    {
                      "--glass-range-progress": `${glassLevelProgress}%`,
                    } as CSSProperties
                  }
                >
                  <div className="appearance-glass-slider-track">
                    <span className="appearance-glass-slider-ticks" aria-hidden>
                      {glassOptions.map(({ id }, index) => (
                        <i
                          key={id}
                          className={
                            index <= glassLevelIndex ? "is-filled" : ""
                          }
                        />
                      ))}
                    </span>
                    <input
                      id="appearance-glass-intensity"
                      className="appearance-glass-range"
                      type="range"
                      min={0}
                      max={glassOptions.length - 1}
                      step={1}
                      value={glassLevelIndex}
                      aria-label={t("prefs.appearance.glass.title")}
                      aria-describedby="appearance-glass-description"
                      aria-valuetext={glassOptions[glassLevelIndex]?.label}
                      onChange={(event) => {
                        const next =
                          glassOptions[Number(event.currentTarget.value)];
                        if (next) setGlassLevel(next.id);
                      }}
                    />
                  </div>
                  <div className="appearance-glass-slider-labels" aria-hidden>
                    {glassOptions.map(({ id, label }) => (
                      <span
                        key={id}
                        className={glassLevel === id ? "is-active" : ""}
                      >
                        {label}
                      </span>
                    ))}
                  </div>
                </div>
              </div>
            </div>
          </section>

          <section className="prefs-card prefs-card--appearance-color">
            <div className="prefs-card-head">
              <div className="prefs-icon-badge" data-tone={tone} aria-hidden>
                <span className="appearance-card-symbol appearance-card-symbol--accent" />
              </div>
              <div>
                <h2 className="prefs-card-title">
                  {t("prefs.appearance.accent.title")}
                </h2>
                <p className="prefs-card-sub">
                  {t("prefs.appearance.accent.sub")}
                </p>
              </div>
            </div>

            <div className="appearance-control-list">
              <div className="appearance-control-row">
                <div className="appearance-control-copy">
                  <strong>{t("prefs.appearance.strategy.title")}</strong>
                  <span>{t("prefs.appearance.strategy.sub")}</span>
                </div>
                <div
                  className="appearance-segmented"
                  role="radiogroup"
                  aria-label={t("prefs.appearance.strategy.title")}
                >
                  {colorStyleOptions.map(({ id, label }) => (
                    <button
                      key={id}
                      type="button"
                      role="radio"
                      aria-checked={colorStyle === id}
                      className={colorStyle === id ? "is-active" : ""}
                      onClick={() => onColorStyleChange(id)}
                    >
                      {label}
                    </button>
                  ))}
                </div>
              </div>

              {colorStyle === "unified" ? (
                <div className="appearance-control-row appearance-control-row--swatches">
                  <div className="appearance-control-copy">
                    <strong>{t("prefs.appearance.strategy.unified")}</strong>
                    <span>{t("prefs.appearance.strategy.unifiedSub")}</span>
                  </div>
                  <div
                    className="shell-color-presets"
                    role="radiogroup"
                    aria-label={t("prefs.colorStyle.presets")}
                  >
                    {SHELL_GRADIENT_PRESETS.map((preset) => {
                      const g = gradientFromPreset(preset.id);
                      const selected = gradient.id === preset.id;
                      return (
                        <button
                          key={preset.id}
                          type="button"
                          role="radio"
                          aria-checked={selected}
                          className={`shell-color-swatch ${selected ? "is-active" : ""}`}
                          style={
                            {
                              "--swatch-bg": gradientSwatchBackground(g),
                              "--swatch-ring": g.primary.color,
                            } as CSSProperties
                          }
                          title={t(preset.labelKey)}
                          aria-label={t(preset.labelKey)}
                          onClick={() => onGradientChange(g)}
                        >
                          <span
                            className="shell-color-swatch-core"
                            aria-hidden
                          />
                        </button>
                      );
                    })}
                    <button
                      type="button"
                      role="radio"
                      aria-checked={gradient.id === "custom"}
                      className={`shell-color-swatch shell-color-swatch--custom ${
                        gradient.id === "custom" ? "is-active" : ""
                      }`}
                      style={
                        {
                          "--swatch-ring": gradient.primary.color,
                        } as CSSProperties
                      }
                      title={t("prefs.colorStyle.custom")}
                      aria-label={t("prefs.colorStyle.custom")}
                      onClick={() => {
                        onBeginCustomGradient();
                        setGradientEditorOpen(true);
                      }}
                    >
                      <span className="shell-color-swatch-core" aria-hidden>
                        <span className="shell-color-swatch-plus">+</span>
                      </span>
                    </button>
                  </div>
                </div>
              ) : null}

              {colorStyle === "dynamic" ? (
                <div className="appearance-control-row">
                  <div className="appearance-control-copy">
                    <strong>{t("prefs.appearance.strategy.dynamic")}</strong>
                    <span>{t("prefs.appearance.strategy.dynamicSub")}</span>
                  </div>
                  <button
                    type="button"
                    className="shell-dynamic-reshuffle"
                    data-tone={tone}
                    onClick={onReshuffleDynamic}
                  >
                    <Dices width={16} height={16} aria-hidden />
                    {t("prefs.colorStyle.reshuffle")}
                  </button>
                </div>
              ) : null}

              {colorStyle === "colorful" ? (
                <div className="appearance-control-row">
                  <div className="appearance-control-copy">
                    <strong>{t("prefs.appearance.strategy.colorful")}</strong>
                    <span>{t("prefs.appearance.strategy.colorfulSub")}</span>
                  </div>
                  <span className="appearance-auto-badge">
                    {t("prefs.appearance.strategy.auto")}
                  </span>
                </div>
              ) : null}
            </div>
          </section>

          <WallpaperSettingsCard controller={wallpaper} tone={tone} />

          <section className="prefs-card morphicon-settings-card">
            <div className="prefs-card-head">
              <button
                type="button"
                className="prefs-icon-badge morphicon-preview-button"
                data-tone={tone}
                onClick={playMorphPreview}
                aria-label={t("prefs.morphicons.preview")}
              >
                <AppMorphIcon
                  icon={morphPreviewActive ? SparklesData : ActivityData}
                  size={22}
                />
              </button>
              <div>
                <h2 className="prefs-card-title">
                  {t("prefs.appearance.motion.title")}
                </h2>
                <p className="prefs-card-sub">
                  {t("prefs.appearance.motion.sub")}
                </p>
              </div>
            </div>

            <div className="morphicon-setting-row">
              <div className="appearance-control-copy">
                <strong>{t("prefs.appearance.motion.spring")}</strong>
                <span>{t("prefs.appearance.motion.springSub")}</span>
              </div>
              <div
                className="morphicon-segmented"
                role="radiogroup"
                aria-label={t("prefs.appearance.motion.spring")}
              >
                {MORPHICON_SPRINGS.map((value) => (
                  <button
                    key={value}
                    type="button"
                    role="radio"
                    aria-checked={spring === value}
                    className={spring === value ? "is-active" : ""}
                    onClick={() => {
                      setSpring(value);
                      playMorphPreview();
                    }}
                  >
                    {spring === value ? (
                      <motion.span
                        layoutId="morphicon-spring-selection"
                        className="morphicon-selection-indicator"
                        transition={
                          reduceMotion
                            ? { duration: 0 }
                            : { type: "spring", bounce: 0, duration: 0.32 }
                        }
                        aria-hidden
                      />
                    ) : null}
                    <span className="morphicon-segmented-label">
                      {t(MORPHICON_SPRING_LABEL[value])}
                    </span>
                  </button>
                ))}
              </div>
            </div>

            <div className="morphicon-setting-row">
              <div className="appearance-control-copy">
                <strong>{t("prefs.appearance.motion.stroke")}</strong>
                <span>{t("prefs.appearance.motion.strokeSub")}</span>
              </div>
              <div
                className="morphicon-segmented"
                role="radiogroup"
                aria-label={t("prefs.appearance.motion.stroke")}
              >
                {MORPHICON_STROKE_WIDTHS.map((value) => (
                  <button
                    key={value}
                    type="button"
                    role="radio"
                    aria-checked={strokeWidth === value}
                    className={strokeWidth === value ? "is-active" : ""}
                    onClick={() => {
                      setStrokeWidth(value);
                      playMorphPreview();
                    }}
                  >
                    {strokeWidth === value ? (
                      <motion.span
                        layoutId="morphicon-stroke-selection"
                        className="morphicon-selection-indicator"
                        transition={
                          reduceMotion
                            ? { duration: 0 }
                            : { type: "spring", bounce: 0, duration: 0.32 }
                        }
                        aria-hidden
                      />
                    ) : null}
                    <span className="morphicon-segmented-label">
                      {t(MORPHICON_STROKE_LABEL[value])}
                    </span>
                  </button>
                ))}
              </div>
            </div>
          </section>

          <section className="prefs-card prefs-card--app-icon">
            <div className="prefs-card-head">
              <div className="prefs-icon-badge" data-tone={tone} aria-hidden>
                <span className="appearance-card-symbol appearance-card-symbol--app">
                  A
                </span>
              </div>
              <div>
                <h2 className="prefs-card-title">{t("prefs.appIcon.title")}</h2>
                <p className="prefs-card-sub">{t("prefs.appIcon.sub")}</p>
              </div>
            </div>

            <div
              className="app-icon-grid"
              role="radiogroup"
              aria-label={t("prefs.appIcon.title")}
            >
              {(appIcon?.options ?? []).map((opt) => (
                <button
                  key={opt.id}
                  type="button"
                  role="radio"
                  aria-checked={appIcon?.current === opt.id}
                  className={`app-icon-option ${appIcon?.current === opt.id ? "active" : ""}`}
                  data-tone={tone}
                  onClick={() => void setAppIcon(opt.id)}
                >
                  <img
                    className="app-icon-thumb"
                    src={opt.dataUrl}
                    alt={appIconLabel(opt.id)}
                  />
                  <span className="app-icon-label">{appIconLabel(opt.id)}</span>
                </button>
              ))}
            </div>
            <p className="prefs-card-note">{t("prefs.appIcon.finderNote")}</p>
          </section>
        </div>

        <div
          className="prefs-category-stack prefs-category-stack--conversation"
          hidden={activeCategory !== "conversation"}
        >
          <section className="prefs-card prefs-card--conversation-display">
            <div className="prefs-card-head">
              <div className="prefs-icon-badge" data-tone={tone} aria-hidden>
                <List width={22} height={22} />
              </div>
              <div>
                <h2 className="prefs-card-title">
                  {t("prefs.chat.layout.title")}
                </h2>
                <p className="prefs-card-sub">{t("prefs.chat.layout.sub")}</p>
              </div>
            </div>

            <div className="prefs-conversation-layout-grid">
              <div className="prefs-conversation-choice-column">
                <div className="prefs-conversation-choice-group">
                  <h3 className="prefs-toggle-heading">
                    {t("prefs.chat.layout.title")}
                  </h3>
                  <div
                    className="theme-options answer-layout-options"
                    role="radiogroup"
                    aria-label={t("prefs.chat.layout.title")}
                  >
                    {answerLayoutOptions.map(
                      ({ id, labelKey, descKey, Icon }) => (
                        <button
                          key={id}
                          type="button"
                          role="radio"
                          aria-checked={prefs.answerLayout === id}
                          className={`theme-option ${prefs.answerLayout === id ? "active" : ""}`}
                          data-tone={tone}
                          onClick={() => onChatAnswerLayoutChange(id)}
                        >
                          <span className="theme-option-icon" aria-hidden>
                            <Icon size={18} strokeWidth={2} />
                          </span>
                          <span className="theme-option-text">
                            <span className="theme-option-label">
                              {t(labelKey)}
                            </span>
                            <span className="theme-option-desc">
                              {t(descKey)}
                            </span>
                          </span>
                          <span className="theme-option-check" aria-hidden />
                        </button>
                      ),
                    )}
                  </div>
                </div>

                <div className="prefs-conversation-choice-group">
                  <h3 className="prefs-toggle-heading">
                    {t("prefs.chat.verbosity")}
                  </h3>
                  <div
                    className="theme-options prefs-conversation-verbosity-options"
                    role="radiogroup"
                    aria-label={t("prefs.chat.verbosity")}
                  >
                    {verbosityOptions.map(({ id, labelKey, descKey }) => (
                      <button
                        key={id}
                        type="button"
                        role="radio"
                        aria-checked={prefs.verbosity === id}
                        className={`theme-option ${prefs.verbosity === id ? "active" : ""}`}
                        data-tone={tone}
                        onClick={() => onChatVerbosityChange(id)}
                      >
                        <span
                          className="theme-option-icon lang-badge"
                          aria-hidden
                        >
                          {id === "compact"
                            ? "简"
                            : id === "normal"
                              ? "常"
                              : "详"}
                        </span>
                        <span className="theme-option-text">
                          <span className="theme-option-label">
                            {t(labelKey)}
                          </span>
                          <span className="theme-option-desc">
                            {t(descKey)}
                          </span>
                        </span>
                        <span className="theme-option-check" aria-hidden />
                      </button>
                    ))}
                  </div>
                </div>
              </div>
              <ConversationLayoutPreview prefs={prefs} />
            </div>
          </section>

          <section className="prefs-card prefs-card--conversation-controls">
            <div className="prefs-card-head">
              <div className="prefs-icon-badge" data-tone={tone} aria-hidden>
                <IconChat width={22} height={22} />
              </div>
              <div>
                <h2 className="prefs-card-title">{t("prefs.chat.title")}</h2>
                <p className="prefs-card-sub">{t("prefs.chat.sub")}</p>
              </div>
            </div>

            <div
              className="prefs-toggle-list"
              role="group"
              aria-labelledby="prefs-chat-details-heading"
            >
              <h3
                id="prefs-chat-details-heading"
                className="prefs-toggle-heading"
              >
                {t("prefs.chat.details")}
              </h3>
              {TOGGLE_KEYS.map(({ key, labelKey, descKey, Icon }) => (
                <div key={key} className="prefs-toggle-row">
                  <span className="prefs-toggle-icon" aria-hidden>
                    <Icon size={15} strokeWidth={2.25} />
                  </span>
                  <span className="prefs-toggle-text">
                    <span className="prefs-toggle-label">{t(labelKey)}</span>
                    <span className="prefs-toggle-desc">{t(descKey)}</span>
                  </span>
                  <button
                    type="button"
                    role="switch"
                    className="prefs-switch"
                    aria-label={t(labelKey)}
                    aria-checked={prefs[key]}
                    data-tone={tone}
                    onClick={() => onChatToggleChange(key, !prefs[key])}
                  >
                    <span className="prefs-switch-thumb" />
                  </button>
                </div>
              ))}
            </div>
          </section>
        </div>

        <div
          className="prefs-category-stack prefs-category-stack--context"
          hidden={activeCategory !== "context"}
        >
          <CompressionSettingsCard tone={tone} />
        </div>

        <div
          className="prefs-category-stack prefs-category-stack--diagnostics"
          hidden={activeCategory !== "diagnostics"}
        >
          <div className="prefs-diag-page-head">
            <p>{t("prefs.diag.pageSub")}</p>
            <button
              type="button"
              className="prefs-diag-btn"
              disabled={busy || diagnosticsStatusBusy}
              onClick={() =>
                void Promise.all([refreshLogs(), refreshDiagnosticsStatus()])
              }
            >
              <RefreshCw
                size={13}
                strokeWidth={2.25}
                className={busy || diagnosticsStatusBusy ? "spin" : undefined}
                aria-hidden
              />
              {busy || diagnosticsStatusBusy
                ? t("prefs.diag.loading")
                : t("prefs.diag.refresh")}
            </button>
          </div>

          <div className="prefs-diag-status-grid">
            {diagnosticCards.map((card) => (
              <DiagnosticStatusCard key={card.id} {...card} />
            ))}
          </div>

          <section className="prefs-card prefs-card--diagnostics">
            <div className="prefs-card-head">
              <div className="prefs-icon-badge" data-tone={tone} aria-hidden>
                <ScrollText width={22} height={22} />
              </div>
              <div className="prefs-diag-card-heading">
                <h2 className="prefs-card-title">{t("prefs.diag.title")}</h2>
                <p className="prefs-card-sub">{t("prefs.diag.sub")}</p>
              </div>
              <button
                type="button"
                className="prefs-diag-btn"
                data-tone={tone}
                disabled={busy || visibleLogRows.length === 0}
                onClick={() => void copyLogs()}
              >
                {t(logsCopied ? "prefs.diag.copied" : "prefs.diag.copyVisible")}
              </button>
            </div>

            <div className="prefs-diag-form">
              <div className="prefs-diag-filter-grid">
                <div className="prefs-diag-group">
                  <span className="prefs-diag-group-label">
                    {t("prefs.diag.scope")}
                  </span>
                  <div
                    className="prefs-chip-row"
                    role="radiogroup"
                    aria-label={t("prefs.diag.scope")}
                  >
                    <button
                      type="button"
                      role="radio"
                      aria-checked={scope === "current"}
                      className={`prefs-chip ${scope === "current" ? "active" : ""}`}
                      data-tone={tone}
                      disabled={!hasSession}
                      title={
                        hasSession
                          ? undefined
                          : t("prefs.diag.scope.currentNone")
                      }
                      onClick={() => setScope("current")}
                    >
                      {t("prefs.diag.scope.current")}
                    </button>
                    <button
                      type="button"
                      role="radio"
                      aria-checked={scope === "all"}
                      className={`prefs-chip ${scope === "all" ? "active" : ""}`}
                      data-tone={tone}
                      onClick={() => setScope("all")}
                    >
                      {t("prefs.diag.scope.all")}
                    </button>
                  </div>
                </div>

                <div className="prefs-diag-group">
                  <span className="prefs-diag-group-label">
                    {t("prefs.diag.source")}
                  </span>
                  <SelectMenu
                    className="prefs-diag-select"
                    value={source}
                    aria-label={t("prefs.diag.source")}
                    onChange={(value) => setSource(value as LogSourceFilter)}
                    options={[
                      { value: "both", label: t("prefs.diag.source.both") },
                      { value: "agent", label: t("prefs.diag.source.agent") },
                      {
                        value: "errors",
                        label: t("prefs.diag.source.errors"),
                      },
                    ]}
                  />
                </div>

                <div className="prefs-diag-group">
                  <span className="prefs-diag-group-label">
                    {t("prefs.diag.level")}
                  </span>
                  <div
                    className="prefs-chip-row"
                    role="radiogroup"
                    aria-label={t("prefs.diag.level")}
                  >
                    <button
                      type="button"
                      role="radio"
                      aria-checked={level === "all"}
                      className={`prefs-chip ${level === "all" ? "active" : ""}`}
                      data-tone={tone}
                      onClick={() => setLevel("all")}
                    >
                      {t("prefs.diag.level.all")}
                    </button>
                    <button
                      type="button"
                      role="radio"
                      aria-checked={level === "issues"}
                      className={`prefs-chip ${level === "issues" ? "active" : ""}`}
                      data-tone={tone}
                      onClick={() => setLevel("issues")}
                    >
                      {t("prefs.diag.level.issues")}
                    </button>
                  </div>
                </div>

                <div className="prefs-diag-group">
                  <span className="prefs-diag-group-label">
                    {t("prefs.diag.lines")}
                  </span>
                  <div
                    className="prefs-chip-row"
                    role="radiogroup"
                    aria-label={t("prefs.diag.lines")}
                  >
                    {LINE_PRESETS.map((count) => (
                      <button
                        key={count}
                        type="button"
                        role="radio"
                        aria-checked={lines === count}
                        className={`prefs-chip ${lines === count ? "active" : ""}`}
                        data-tone={tone}
                        onClick={() => setLines(count)}
                      >
                        {count}
                      </button>
                    ))}
                  </div>
                </div>
              </div>

              <div className="prefs-diag-toolbar">
                <label className="prefs-diag-search">
                  <Search size={14} strokeWidth={2.2} aria-hidden />
                  <input
                    type="search"
                    aria-label={t("prefs.diag.search")}
                    value={logSearch}
                    placeholder={t("prefs.diag.search.ph")}
                    onChange={(event) => setLogSearch(event.target.value)}
                  />
                </label>
                <div className="prefs-diag-actions">
                  <button
                    type="button"
                    className="prefs-diag-link"
                    onClick={() => setShowAdvanced((value) => !value)}
                    aria-expanded={showAdvanced}
                  >
                    {showAdvanced
                      ? t("prefs.diag.advanced.hide")
                      : t("prefs.diag.advanced.show")}
                  </button>
                </div>
              </div>

              {showAdvanced && (
                <div className="prefs-diag-advanced">
                  <label className="prefs-diag-row">
                    <span className="prefs-diag-label">
                      {t("prefs.diag.session")}
                    </span>
                    <input
                      className="prefs-diag-input"
                      type="text"
                      value={manualSession}
                      placeholder={t("prefs.diag.session.ph")}
                      onChange={(event) => setManualSession(event.target.value)}
                      spellCheck={false}
                      autoComplete="off"
                    />
                  </label>
                  <label className="prefs-diag-row">
                    <span className="prefs-diag-label">
                      {t("prefs.diag.turn")}
                    </span>
                    <input
                      className="prefs-diag-input"
                      type="text"
                      value={turnId}
                      placeholder={t("prefs.diag.turn.ph")}
                      onChange={(event) => setTurnId(event.target.value)}
                      spellCheck={false}
                      autoComplete="off"
                    />
                  </label>
                </div>
              )}

              {errorMsg && (
                <p className="prefs-diag-error" role="alert">
                  {errorMsg}
                </p>
              )}
              {queried && !errorMsg && rows.length === 0 && (
                <p className="prefs-diag-empty">{t("prefs.diag.empty")}</p>
              )}
              {queried &&
                !errorMsg &&
                rows.length > 0 &&
                visibleLogRows.length === 0 && (
                  <p className="prefs-diag-empty">
                    {t("prefs.diag.emptySearch")}
                  </p>
                )}
              {visibleLogRows.length > 0 && (
                <div className="prefs-diag-results">
                  <div
                    className="prefs-diag-results-head"
                    role="status"
                    aria-live="polite"
                  >
                    <span>
                      {t("prefs.diag.results", {
                        shown: String(visibleLogRows.length),
                        total: String(rows.length),
                      })}
                    </span>
                    <span>{t("prefs.diag.newestFirst")}</span>
                  </div>
                  <ul className="prefs-diag-log prefs-diag-log-list">
                    {visibleLogRows.map((row, index) => {
                      const severity = diagnosticLogLevel(row.raw);
                      return (
                        <li
                          key={row.source + "-" + index}
                          className={"prefs-diag-log-row is-" + severity}
                        >
                          <span
                            className={"prefs-diag-source is-" + row.source}
                          >
                            {row.source}
                          </span>
                          <code>{row.raw}</code>
                        </li>
                      );
                    })}
                  </ul>
                </div>
              )}
            </div>
          </section>

          <section className="prefs-card prefs-diag-export-card">
            <div className="prefs-icon-badge" data-tone={tone} aria-hidden>
              <Download size={20} />
            </div>
            <div className="prefs-diag-export-copy">
              <h2 className="prefs-card-title">
                {t("prefs.diag.export.title")}
              </h2>
              <p className="prefs-card-sub">{t("prefs.diag.export.sub")}</p>
              {diagnosticsExportPath ? (
                <small>
                  {t("prefs.diag.export.done", {
                    path: diagnosticsExportPath,
                  })}
                </small>
              ) : null}
            </div>
            <button
              type="button"
              className="prefs-diag-btn primary"
              disabled={exportingDiagnostics}
              onClick={() => void exportDiagnostics()}
            >
              {t(
                exportingDiagnostics
                  ? "prefs.diag.export.exporting"
                  : "prefs.diag.export.action",
              )}
            </button>
          </section>
        </div>

        <div
          className="prefs-category-stack prefs-category-stack--general"
          hidden={activeCategory !== "general"}
        >
          <section className="prefs-card prefs-card--language">
            <div className="prefs-card-head">
              <div className="prefs-icon-badge" data-tone={tone} aria-hidden>
                <IconGlobe width={22} height={22} />
              </div>
              <div>
                <h2 className="prefs-card-title">{t("prefs.lang.title")}</h2>
                <p className="prefs-card-sub">{t("prefs.lang.sub")}</p>
              </div>
            </div>

            <div
              className="theme-options lang-options"
              role="radiogroup"
              aria-label={t("prefs.lang.title")}
            >
              {langOptions.map(({ id, label, desc }) => (
                <button
                  key={id}
                  type="button"
                  role="radio"
                  aria-checked={locale === id}
                  className={`theme-option ${locale === id ? "active" : ""}`}
                  data-tone={tone}
                  onClick={() => setLocale(id)}
                >
                  <span className="theme-option-icon lang-badge" aria-hidden>
                    {id === "zh" ? "中" : "En"}
                  </span>
                  <span className="theme-option-text">
                    <span className="theme-option-label">{label}</span>
                    <span className="theme-option-desc">{desc}</span>
                  </span>
                  <span className="theme-option-check" aria-hidden />
                </button>
              ))}
            </div>
          </section>

          <section className="prefs-card prefs-card--system">
            <div className="prefs-card-head">
              <div className="prefs-icon-badge" data-tone={tone} aria-hidden>
                <Wrench width={22} height={22} />
              </div>
              <div>
                <h2 className="prefs-card-title">
                  {t("prefs.system.title" as never)}
                </h2>
                <p className="prefs-card-sub">
                  {t("prefs.system.sub" as never)}
                </p>
              </div>
            </div>

            <div
              className="prefs-toggle-list"
              role="group"
              aria-label={t("prefs.system.title" as never)}
            >
              <label className="prefs-toggle-row">
                <span className="prefs-toggle-icon" aria-hidden>
                  <Play size={15} strokeWidth={2.25} />
                </span>
                <span className="prefs-toggle-text">
                  <span className="prefs-toggle-label">
                    {t("prefs.system.autostart" as never)}
                  </span>
                  <span className="prefs-toggle-desc">
                    {t("prefs.system.autostartDesc" as never)}
                  </span>
                </span>
                <AutostartSwitch tone={tone} />
              </label>
              <SidebarVisibleSetting tone={tone} />
            </div>
          </section>
        </div>

        <div
          className="prefs-category-stack prefs-category-stack--about"
          hidden={activeCategory !== "about"}
        >
          <section className="prefs-card prefs-card--about-hero">
            <div className="prefs-about-hero">
              <div className="prefs-about-icon" aria-hidden>
                <img
                  src={selectedAppIcon?.dataUrl ?? appIconAsset}
                  alt=""
                  width={82}
                  height={82}
                />
              </div>
              <h2 className="prefs-about-title">Astro</h2>
              <p className="prefs-about-tagline">{t("about.tagline")}</p>
              <div className="prefs-about-badges">
                <span data-tone>{appMeta.version}</span>
                <span>{aboutCopy.platform}</span>
                <span data-status="stable">
                  {import.meta.env.DEV
                    ? aboutCopy.development
                    : aboutCopy.stable}
                </span>
              </div>
            </div>
          </section>

          <section className="prefs-card prefs-card--about-update">
            <div className="prefs-card-head">
              <div className="prefs-icon-badge" data-tone={tone} aria-hidden>
                <ArrowUp size={21} />
              </div>
              <div>
                <h2 className="prefs-card-title">{aboutCopy.updateTitle}</h2>
                <p className="prefs-card-sub">{aboutCopy.updateSub}</p>
              </div>
            </div>
            <div className="prefs-about-setting-list">
              <div className="prefs-about-setting-row">
                <span className="prefs-about-setting-copy">
                  <strong>{t("about.version", { v: appMeta.version })}</strong>
                  <small>
                    {appMeta.runtime} · {appMeta.identifier}
                  </small>
                </span>
                <button
                  type="button"
                  disabled={
                    updatePhase === "checking" ||
                    updatePhase === "installing" ||
                    updatePhase === "unconfigured"
                  }
                  onClick={() =>
                    void (updatePhase === "available"
                      ? installUpdate()
                      : checkForUpdate())
                  }
                >
                  {updatePhase === "available" ? (
                    <Download size={13} aria-hidden />
                  ) : (
                    <RefreshCw size={13} aria-hidden />
                  )}
                  {updatePhase === "available"
                    ? aboutCopy.install
                    : updatePhase === "error"
                      ? aboutCopy.retry
                      : aboutCopy.updates}
                </button>
              </div>
              <p
                className="prefs-about-update-status"
                role={updatePhase === "error" ? "alert" : "status"}
              >
                {updateStatus}
              </p>
              {updateInfo?.notes ? (
                <p className="prefs-about-update-notes">{updateInfo.notes}</p>
              ) : null}
            </div>
          </section>

          <section className="prefs-card prefs-card--about-project">
            <div className="prefs-card-head">
              <div className="prefs-icon-badge" data-tone={tone} aria-hidden>
                <Copyright size={21} />
              </div>
              <div>
                <h2 className="prefs-card-title">{aboutCopy.projectTitle}</h2>
                <p className="prefs-card-sub">{aboutCopy.projectSub}</p>
              </div>
            </div>
            <div className="prefs-about-setting-list">
              <div className="prefs-about-setting-row">
                <span className="prefs-about-setting-copy">
                  <strong>{aboutCopy.license}</strong>
                  {licenseExpanded ? (
                    <small>{aboutCopy.licenseValue}</small>
                  ) : null}
                </span>
                <button
                  type="button"
                  aria-expanded={licenseExpanded}
                  onClick={() => setLicenseExpanded((value) => !value)}
                >
                  {licenseExpanded ? aboutCopy.collapse : aboutCopy.view}
                </button>
              </div>
              <div className="prefs-about-setting-row">
                <span className="prefs-about-setting-copy">
                  <strong>{aboutCopy.dataDirectory}</strong>
                </span>
                <code>~/.astro</code>
              </div>
            </div>
          </section>
        </div>
      </div>
      <ShellGradientEditor
        open={gradientEditorOpen}
        initial={gradient}
        onPreview={onPreviewGradient}
        onConfirm={(g) => {
          onCommitCustomGradient(g);
          setGradientEditorOpen(false);
        }}
        onCancel={() => {
          onCancelCustomGradient();
          setGradientEditorOpen(false);
        }}
      />
    </div>
  );
}

function SidebarVisibleSetting({ tone }: { tone?: string }) {
  const [count, setCount] = useState(() => {
    try {
      const v = localStorage.getItem("astro:sidebar-visible-sessions");
      if (v) {
        const n = Number(v);
        if (n >= 1 && n <= 50) return n;
      }
    } catch {}
    return 5;
  });

  return (
    <label className="prefs-toggle-row">
      <span className="prefs-toggle-icon" aria-hidden>
        <List size={15} strokeWidth={2.25} />
      </span>
      <span className="prefs-toggle-text">
        <span className="prefs-toggle-label">侧栏默认显示会话数</span>
        <span className="prefs-toggle-desc">超出部分折叠，点击展开</span>
      </span>
      <select
        className="prefs-select"
        value={count}
        data-tone={tone}
        onChange={(e) => {
          const n = Number(e.target.value);
          setCount(n);
          try {
            localStorage.setItem("astro:sidebar-visible-sessions", String(n));
          } catch {}
        }}
      >
        {[3, 5, 8, 10, 15, 20].map((n) => (
          <option key={n} value={n}>
            {n} 条
          </option>
        ))}
      </select>
    </label>
  );
}
