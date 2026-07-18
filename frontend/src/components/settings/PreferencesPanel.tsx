/** 偏好设置（主题、语言、日志诊断、关于）。 */
import { useEffect, useRef, useState } from "react";
import type { LucideIcon } from "lucide-react";
import {
  Activity,
  Brain,
  Clock,
  Plug,
  ScrollText,
  Sparkles,
  Webhook,
  Wrench,
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { useAppIcon } from "../../hooks/settings/useAppIcon";
import type { AppIconId } from "../../types";
import type { ThemeMode } from "../../hooks/app/useTheme";
import type { ChatDisplayPrefs, ChatVerbosity } from "../../hooks/chat/useChatDisplayPrefs";
import { useI18n } from "../../i18n/LocaleContext";
import type { Locale, MessageKey } from "../../i18n/messages";
import { IconGlobe, IconMonitor, IconMoon, IconSun, IconChat, IconAtom } from "../icons/NavIcons";
import { SelectMenu } from "../ui/SelectMenu";
import CompressionSettingsCard from "./CompressionSettingsCard";

/** 查询返回的单行日志 */
type AgentLogLine = { raw: string; source: string };

/** 日志来源过滤 */
type LogSourceFilter = "both" | "agent" | "errors";

/** 查询范围：本次会话 / 全部会话 */
type LogScope = "current" | "all";

/** 内容过滤：全部 / 只看问题（warn 及以上） */
type LogLevelFilter = "all" | "issues";

/** 行数预设 */
const LINE_PRESETS = [50, 100, 200] as const;

/** 偏好设置入参 */
type Props = {
  /** 当前主题模式 */
  mode: ThemeMode;
  onChange: (mode: ThemeMode) => void;
  /** 当前 tab 主题色：blue | green | purple | orange | pink */
  tone?: string;
  chatDisplayPrefs: ChatDisplayPrefs;
  onChatVerbosityChange: (verbosity: ChatVerbosity) => void;
  onChatToggleChange: (
    key: keyof Omit<ChatDisplayPrefs, "verbosity">,
    value: boolean,
  ) => void;
  /** 当前聊天会话 ID，用于预填诊断过滤 */
  activeSessionId?: string;
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
  tone = "pink",
  chatDisplayPrefs: prefs,
  onChatVerbosityChange,
  onChatToggleChange,
  activeSessionId,
}: Props) {
  const { locale, setLocale, t } = useI18n();
  const { settings: appIcon, setIcon: setAppIcon } = useAppIcon();

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
    <div className="prefs-page" data-tone={tone}>
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
      </section>

      <CompressionSettingsCard tone={tone} />

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

      <section className="prefs-card">
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
  );
}
