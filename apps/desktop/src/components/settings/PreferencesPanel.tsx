/** 偏好设置（主题、语言、日志诊断、关于）。 */
import { useEffect, useRef, useState, type CSSProperties } from "react";
import type { LucideIcon } from "lucide-react";
import {
  Activity,
  Blend,
  Brain,
  Clock,
  Dices,
  Palette,
  Play,
  Plug,
  ScrollText,
  Sparkles,
  Webhook,
  Layers,
  List,
  Wrench,
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { useAppIcon } from "../../hooks/settings/useAppIcon";
import type { AppIconId } from "../../types";
import type { ShellColorStyle } from "../../hooks/app/useShellColorStyle";
import { useTheme, type ThemeMode, type GlassLevel } from "../../hooks/app/useTheme";
import type { ChatDisplayPrefs, ChatVerbosity } from "../../hooks/chat/useChatDisplayPrefs";
import { useI18n } from "../../i18n/LocaleContext";
import type { Locale, MessageKey } from "../../i18n/messages";
import {
  gradientFromPreset,
  gradientSwatchBackground,
  SHELL_GRADIENT_PRESETS,
  type ShellGradient,
} from "../../lib/ui/shellGradient";
import { IconGlobe, IconMonitor, IconMoon, IconSun, IconChat, IconAtom } from "../icons/NavIcons";
import { SelectMenu } from "../ui/SelectMenu";
import CompressionSettingsCard from "./CompressionSettingsCard";
import ShellGradientEditor from "./ShellGradientEditor";

/** 查询返回的单行日志 */
type AgentLogLine = { raw: string; source: string };

/** 日志来源过滤 */
type LogSourceFilter = "both" | "agent" | "errors";

/** 查询范围：本次会话 / 全部会话 */
type LogScope = "current" | "all";

/** 内容过滤：全部 / 只看问题（warn 及以上） */
type LogLevelFilter = "all" | "issues";


function AutostartSwitch({ tone }: { tone: string }) {
  const [enabled, setEnabled] = useState(false);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    import("@tauri-apps/plugin-autostart").then((mod) => {
      mod.isEnabled().then((v) => { setEnabled(v); setLoading(false); }).catch(() => setLoading(false));
    }).catch(() => setLoading(false));
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
const LINE_PRESETS = [50, 100, 200] as const;

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
  tone?: string;
  chatDisplayPrefs: ChatDisplayPrefs;
  onChatVerbosityChange: (verbosity: ChatVerbosity) => void;
  onChatToggleChange: (
    key: keyof Omit<ChatDisplayPrefs, "verbosity">,
    value: boolean,
  ) => void;
  activeSessionId?: string;
  /** 由外部 settings 侧栏控制显示哪个分类；未传则显示内部导航 */
  section?: PreferenceCategory;
};

/** 聊天展示开关字段（不含 verbosity） */
type ToggleKey = keyof Omit<ChatDisplayPrefs, "verbosity">;

const TOGGLE_KEYS: {
  key: ToggleKey;
  labelKey: MessageKey;
  descKey: MessageKey;
  Icon: LucideIcon;
}[] = [
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
  tone = "twilight",
  chatDisplayPrefs: prefs,
  onChatVerbosityChange,
  onChatToggleChange,
  activeSessionId,
  section,
}: Props) {
  const { locale, setLocale, t } = useI18n();
  const { glassLevel, setGlassLevel } = useTheme();
  const { settings: appIcon, setIcon: setAppIcon } = useAppIcon();
  const [gradientEditorOpen, setGradientEditorOpen] = useState(false);
  const [internalCategory, setInternalCategory] =
    useState<PreferenceCategory>("general");
  const activeCategory = section ?? internalCategory;
  const setActiveCategory = section ? () => {} : setInternalCategory;
  const showInternalNav = !section;

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
  const [rows, setRows] = useState<AgentLogLine[]>([]);
  const [busy, setBusy] = useState(false);
  const [errorMsg, setErrorMsg] = useState("");
  const [queried, setQueried] = useState(false);

  const themeOptions: {
    id: ThemeMode;
    label: string;
    desc: string;
    Icon: typeof IconSun;
  }[] = [
    {
      id: "light",
      label: t("prefs.theme.light"),
      desc: t("prefs.theme.lightDesc"),
      Icon: IconSun,
    },
    {
      id: "dark",
      label: t("prefs.theme.dark"),
      desc: t("prefs.theme.darkDesc"),
      Icon: IconMoon,
    },
    {
      id: "auto",
      label: t("prefs.theme.auto"),
      desc: t("prefs.theme.autoDesc"),
      Icon: IconMonitor,
    },
  ];

  const colorStyleOptions: {
    id: ShellColorStyle;
    label: string;
    desc: string;
    Icon: LucideIcon;
  }[] = [
    {
      id: "colorful",
      label: t("prefs.colorStyle.colorful"),
      desc: t("prefs.colorStyle.colorfulDesc"),
      Icon: Palette,
    },
    {
      id: "unified",
      label: t("prefs.colorStyle.unified"),
      desc: t("prefs.colorStyle.unifiedDesc"),
      Icon: Blend,
    },
    {
      id: "dynamic",
      label: t("prefs.colorStyle.dynamic"),
      desc: t("prefs.colorStyle.dynamicDesc"),
      Icon: Sparkles,
    },
  ];

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

  const ModeIcon =
    themeOptions.find((o) => o.id === mode)?.Icon ?? IconSun;
  const ColorStyleIcon =
    colorStyleOptions.find((o) => o.id === colorStyle)?.Icon ?? Palette;
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
      manual || (scope === "current" ? activeSessionId ?? null : null);
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
    } catch (e) {
      setRows([]);
      setQueried(true);
      setErrorMsg(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }
  refreshRef.current = refreshLogs;

  useEffect(() => {
    const timer = setTimeout(() => {
      void refreshRef.current();
    }, 250);
    return () => clearTimeout(timer);
  }, [scope, level, lines, source, manualSession, turnId]);

  async function copyLogs() {
    const text = rows.map((r) => `[${r.source}] ${r.raw}`).join("\n");
    try {
      await navigator.clipboard.writeText(text);
    } catch (e) {
      setErrorMsg(e instanceof Error ? e.message : String(e));
    }
  }

  return (
    <div className={`prefs-page ${section ? "is-embedded" : ""}`} data-tone={tone}>
      <nav className="prefs-category-nav" aria-label={t("prefs.category.aria")} hidden={!showInternalNav}>
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
        className="prefs-category-stack"
        hidden={activeCategory !== "appearance"}
      >
      <section className="prefs-card">
        <div className="prefs-card-head">
          <div className="prefs-icon-badge" data-tone={tone} aria-hidden>
            <ModeIcon width={22} height={22} />
          </div>
          <div>
            <h2 className="prefs-card-title">{t("prefs.theme.title")}</h2>
            <p className="prefs-card-sub">{t("prefs.theme.sub")}</p>
          </div>
        </div>

        <div
          className="theme-options"
          role="radiogroup"
          aria-label={t("prefs.theme.title")}
        >
          {themeOptions.map(({ id, label, desc, Icon }) => (
            <button
              key={id}
              type="button"
              role="radio"
              aria-checked={mode === id}
              className={`theme-option ${mode === id ? "active" : ""}`}
              data-tone={tone}
              onClick={() => onChange(id)}
            >
              <span className="theme-option-icon" aria-hidden>
                <Icon />
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

      <section className="prefs-card">
        <div className="prefs-card-head">
          <div className="prefs-icon-badge" data-tone={tone} aria-hidden>
            <Layers size={22} />
          </div>
          <div>
            <h2 className="prefs-card-title">{t("prefs.glass.title")}</h2>
            <p className="prefs-card-sub">{t("prefs.glass.sub")}</p>
          </div>
        </div>
        <div className="theme-options" role="radiogroup" aria-label={t("prefs.glass.title")}>
          {([
            { id: "rich" as GlassLevel, label: t("prefs.glass.rich"), desc: t("prefs.glass.richDesc") },
            { id: "normal" as GlassLevel, label: t("prefs.glass.normal"), desc: t("prefs.glass.normalDesc") },
            { id: "minimal" as GlassLevel, label: t("prefs.glass.minimal"), desc: t("prefs.glass.minimalDesc") },
          ]).map(({ id, label, desc }) => (
            <button
              key={id}
              type="button"
              role="radio"
              aria-checked={glassLevel === id}
              className={`theme-option ${glassLevel === id ? "active" : ""}`}
              data-tone={tone}
              onClick={() => setGlassLevel(id)}
            >
              <span className="theme-option-text">
                <span className="theme-option-label">{label}</span>
                <span className="theme-option-desc">{desc}</span>
              </span>
              <span className="theme-option-check" aria-hidden />
            </button>
          ))}
        </div>
      </section>

      <section className="prefs-card">
        <div className="prefs-card-head">
          <div className="prefs-icon-badge" data-tone={tone} aria-hidden>
            <ColorStyleIcon width={22} height={22} />
          </div>
          <div>
            <h2 className="prefs-card-title">{t("prefs.colorStyle.title")}</h2>
            <p className="prefs-card-sub">{t("prefs.colorStyle.sub")}</p>
          </div>
        </div>

        <div
          className="theme-options"
          role="radiogroup"
          aria-label={t("prefs.colorStyle.title")}
        >
          {colorStyleOptions.map(({ id, label, desc, Icon }) => (
            <button
              key={id}
              type="button"
              role="radio"
              aria-checked={colorStyle === id}
              className={`theme-option ${colorStyle === id ? "active" : ""}`}
              data-tone={tone}
              onClick={() => onColorStyleChange(id)}
            >
              <span className="theme-option-icon" aria-hidden>
                <Icon />
              </span>
              <span className="theme-option-text">
                <span className="theme-option-label">{label}</span>
                <span className="theme-option-desc">{desc}</span>
              </span>
              <span className="theme-option-check" aria-hidden />
            </button>
          ))}
        </div>

        {colorStyle === "unified" ? (
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
                  <span className="shell-color-swatch-core" aria-hidden />
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
        ) : null}

        {colorStyle === "dynamic" ? (
          <div className="shell-dynamic-actions">
            <p className="shell-dynamic-hint">{t("prefs.colorStyle.dynamicHint")}</p>
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
      </section>

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

      <section className="prefs-card">
        <div className="prefs-card-head">
          <div className="prefs-icon-badge" data-tone={tone} aria-hidden>
            <IconAtom width={22} height={22} />
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
              <img className="app-icon-thumb" src={opt.dataUrl} alt={appIconLabel(opt.id)} />
              <span className="app-icon-label">{appIconLabel(opt.id)}</span>
            </button>
          ))}
        </div>
        <p className="prefs-card-note">{t("prefs.appIcon.finderNote")}</p>
      </section>
      </div>

      <div
        className="prefs-category-stack"
        hidden={activeCategory !== "conversation"}
      >
      <section className="prefs-card">
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
          className="theme-options"
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
              <span className="theme-option-icon lang-badge" aria-hidden>
                {id === "compact" ? "简" : id === "normal" ? "常" : "详"}
              </span>
              <span className="theme-option-text">
                <span className="theme-option-label">{t(labelKey)}</span>
                <span className="theme-option-desc">{t(descKey)}</span>
              </span>
              <span className="theme-option-check" aria-hidden />
            </button>
          ))}
        </div>

        <div className="prefs-toggle-list" role="group" aria-label={t("prefs.chat.details")}>
          <h3 className="prefs-toggle-heading">{t("prefs.chat.details")}</h3>
          {TOGGLE_KEYS.map(({ key, labelKey, descKey, Icon }) => (
            <label key={key} className="prefs-toggle-row">
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
                aria-checked={prefs[key]}
                data-tone={tone}
                onClick={() => onChatToggleChange(key, !prefs[key])}
              >
                <span className="prefs-switch-thumb" />
              </button>
            </label>
          ))}
        </div>

        <div className="prefs-toggle-list" role="group" aria-label="侧栏显示">
          <h3 className="prefs-toggle-heading">侧栏</h3>
          <SidebarVisibleSetting tone={tone} />
        </div>
      </section>

      </div>

      <div
        className="prefs-category-stack"
        hidden={activeCategory !== "context"}
      >
      <CompressionSettingsCard tone={tone} />
      </div>

      <div
        className="prefs-category-stack"
        hidden={activeCategory !== "diagnostics"}
      >
      <section className="prefs-card">
        <div className="prefs-card-head">
          <div className="prefs-icon-badge" data-tone={tone} aria-hidden>
            <ScrollText width={22} height={22} />
          </div>
          <div>
            <h2 className="prefs-card-title">{t("prefs.diag.title")}</h2>
            <p className="prefs-card-sub">{t("prefs.diag.sub")}</p>
          </div>
        </div>

        <div className="prefs-diag-form">
          <div className="prefs-diag-quick">
            <div className="prefs-diag-group">
              <span className="prefs-diag-group-label">{t("prefs.diag.scope")}</span>
              <div className="prefs-chip-row" role="radiogroup" aria-label={t("prefs.diag.scope")}>
                <button
                  type="button"
                  role="radio"
                  aria-checked={scope === "current"}
                  className={`prefs-chip ${scope === "current" ? "active" : ""}`}
                  data-tone={tone}
                  disabled={!hasSession}
                  title={hasSession ? undefined : t("prefs.diag.scope.currentNone")}
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
              <span className="prefs-diag-group-label">{t("prefs.diag.level")}</span>
              <div className="prefs-chip-row" role="radiogroup" aria-label={t("prefs.diag.level")}>
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
              <span className="prefs-diag-group-label">{t("prefs.diag.lines")}</span>
              <div className="prefs-chip-row" role="radiogroup" aria-label={t("prefs.diag.lines")}>
                {LINE_PRESETS.map((n) => (
                  <button
                    key={n}
                    type="button"
                    role="radio"
                    aria-checked={lines === n}
                    className={`prefs-chip ${lines === n ? "active" : ""}`}
                    data-tone={tone}
                    onClick={() => setLines(n)}
                  >
                    {n}
                  </button>
                ))}
              </div>
            </div>
          </div>

          <div className="prefs-diag-actions">
            <button
              type="button"
              className="prefs-diag-btn primary"
              data-tone={tone}
              disabled={busy}
              onClick={() => void refreshLogs()}
            >
              {busy ? t("prefs.diag.loading") : t("prefs.diag.refresh")}
            </button>
            <button
              type="button"
              className="prefs-diag-btn"
              data-tone={tone}
              disabled={busy || rows.length === 0}
              onClick={() => void copyLogs()}
            >
              {t("prefs.diag.copy")}
            </button>
            <button
              type="button"
              className="prefs-diag-link"
              onClick={() => setShowAdvanced((v) => !v)}
              aria-expanded={showAdvanced}
            >
              {showAdvanced ? t("prefs.diag.advanced.hide") : t("prefs.diag.advanced.show")}
            </button>
          </div>

          {showAdvanced && (
            <div className="prefs-diag-advanced">
              <label className="prefs-diag-row">
                <span className="prefs-diag-label">{t("prefs.diag.session")}</span>
                <input
                  className="prefs-diag-input"
                  type="text"
                  value={manualSession}
                  placeholder={t("prefs.diag.session.ph")}
                  onChange={(e) => setManualSession(e.target.value)}
                  spellCheck={false}
                  autoComplete="off"
                />
              </label>
              <label className="prefs-diag-row">
                <span className="prefs-diag-label">{t("prefs.diag.turn")}</span>
                <input
                  className="prefs-diag-input"
                  type="text"
                  value={turnId}
                  placeholder={t("prefs.diag.turn.ph")}
                  onChange={(e) => setTurnId(e.target.value)}
                  spellCheck={false}
                  autoComplete="off"
                />
              </label>
              <label className="prefs-diag-row">
                <span className="prefs-diag-label">{t("prefs.diag.source")}</span>
                <SelectMenu
                  className="prefs-diag-select"
                  value={source}
                  aria-label={t("prefs.diag.source")}
                  onChange={(v) => setSource(v as LogSourceFilter)}
                  options={[
                    { value: "both", label: t("prefs.diag.source.both") },
                    { value: "agent", label: t("prefs.diag.source.agent") },
                    { value: "errors", label: t("prefs.diag.source.errors") },
                  ]}
                />
              </label>
            </div>
          )}

          {errorMsg && <p className="prefs-diag-error">{errorMsg}</p>}
          {queried && !errorMsg && rows.length === 0 && (
            <p className="prefs-diag-empty">{t("prefs.diag.empty")}</p>
          )}
          {rows.length > 0 && (
            <pre className="prefs-diag-log">
              {rows.map((r) => `[${r.source}] ${r.raw}`).join("\n")}
            </pre>
          )}
        </div>
      </section>
      </div>

      <div
        className="prefs-category-stack prefs-category-stack--general"
        hidden={activeCategory !== "general"}
      >
      <section className="prefs-card prefs-card--general">
        <div className="prefs-general-group">
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
        </div>

        <div className="prefs-general-divider" aria-hidden />

        <div className="prefs-general-group">
          <div className="prefs-card-head">
            <div className="prefs-icon-badge" data-tone={tone} aria-hidden>
              <Wrench width={22} height={22} />
            </div>
            <div>
              <h2 className="prefs-card-title">{t("prefs.system.title" as never)}</h2>
              <p className="prefs-card-sub">{t("prefs.system.sub" as never)}</p>
            </div>
          </div>

          <div className="prefs-toggle-list" role="group" aria-label={t("prefs.system.title" as never)}>
            <label className="prefs-toggle-row">
              <span className="prefs-toggle-icon" aria-hidden>
                <Play size={15} strokeWidth={2.25} />
              </span>
              <span className="prefs-toggle-text">
                <span className="prefs-toggle-label">{t("prefs.system.autostart" as never)}</span>
                <span className="prefs-toggle-desc">{t("prefs.system.autostartDesc" as never)}</span>
              </span>
              <AutostartSwitch tone={tone} />
            </label>
          </div>
        </div>
      </section>
      </div>

      <div
        className="prefs-category-stack"
        hidden={activeCategory !== "about"}
      >
      <section className="prefs-card">
        <div className="prefs-card-head">
          <div className="prefs-icon-badge" data-tone={tone} aria-hidden>
            <IconAtom width={22} height={22} />
          </div>
          <div>
            <h2 className="prefs-card-title">{t("prefs.app.aboutTitle")}</h2>
            <p className="prefs-card-sub">{t("prefs.app.about")}</p>
          </div>
        </div>
      </section>
      </div>
      </div>
    </div>
  );
}

function SidebarVisibleSetting({ tone }: { tone?: string }) {
  const [count, setCount] = useState(() => {
    try {
      const v = localStorage.getItem("astro:sidebar-visible-sessions");
      if (v) { const n = Number(v); if (n >= 1 && n <= 50) return n; }
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
          try { localStorage.setItem("astro:sidebar-visible-sessions", String(n)); } catch {}
        }}
      >
        {[3, 5, 8, 10, 15, 20].map((n) => (
          <option key={n} value={n}>{n} 条</option>
        ))}
      </select>
    </label>
  );
}
