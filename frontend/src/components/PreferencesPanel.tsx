/** 偏好设置（主题、语言、日志诊断、关于）。 */
import { useState } from "react";
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
import type { ThemeMode } from "../hooks/useTheme";
import type { ChatDisplayPrefs, ChatVerbosity } from "../hooks/useChatDisplayPrefs";
import { useI18n } from "../i18n/LocaleContext";
import type { Locale, MessageKey } from "../i18n/messages";
import { IconGlobe, IconMonitor, IconMoon, IconSun, IconChat, IconAtom } from "./icons/NavIcons";
import { SelectMenu } from "./ui/SelectMenu";

/** 查询返回的单行日志 */
type AgentLogLine = { raw: string; source: string };

/** 日志来源过滤 */
type LogSourceFilter = "both" | "agent" | "errors";

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

  const [sessionId, setSessionId] = useState(activeSessionId ?? "");
  const [turnId, setTurnId] = useState("");
  const [source, setSource] = useState<LogSourceFilter>("both");
  const [lines, setLines] = useState(50);
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

  async function refreshLogs() {
    setBusy(true);
    setErrorMsg("");
    try {
      const result = await invoke<AgentLogLine[]>("query_agent_logs", {
        sessionId: sessionId.trim() || null,
        turnId: turnId.trim() || null,
        source,
        lines,
        minLevel: null,
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
          <label className="prefs-diag-row">
            <span className="prefs-diag-label">{t("prefs.diag.session")}</span>
            <input
              className="prefs-diag-input"
              type="text"
              value={sessionId}
              onChange={(e) => setSessionId(e.target.value)}
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
          <label className="prefs-diag-row">
            <span className="prefs-diag-label">{t("prefs.diag.lines")}</span>
            <input
              className="prefs-diag-input"
              type="number"
              min={1}
              max={500}
              value={lines}
              onChange={(e) => {
                const n = Number(e.target.value);
                setLines(Number.isFinite(n) ? Math.max(1, Math.min(500, n)) : 50);
              }}
            />
          </label>

          <div className="prefs-diag-actions">
            <button
              type="button"
              className="prefs-diag-btn primary"
              data-tone={tone}
              disabled={busy}
              onClick={() => void refreshLogs()}
            >
              {t("prefs.diag.refresh")}
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
          </div>

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
