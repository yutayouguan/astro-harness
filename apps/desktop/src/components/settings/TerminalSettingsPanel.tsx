import { RotateCcw, ShieldCheck, TerminalSquare, Type } from "lucide-react";
import { useCallback, useState } from "react";

import { useI18n } from "../../i18n/LocaleContext";
import {
  DEFAULT_TERMINAL_SETTINGS,
  TERMINAL_FONT_PRESETS,
  readTerminalSettings,
  saveTerminalSettings,
  type TerminalSettings,
} from "../../lib/terminal/terminalSettings";

type Props = {
  tone?: string;
};

export default function TerminalSettingsPanel({ tone = "twilight" }: Props) {
  const { t } = useI18n();
  const [settings, setSettings] = useState(readTerminalSettings);

  const update = useCallback((patch: Partial<TerminalSettings>) => {
    setSettings((current) => saveTerminalSettings({ ...current, ...patch }));
  }, []);

  const selectedPreset =
    TERMINAL_FONT_PRESETS.find((preset) => preset.value === settings.fontFamily)
      ?.value ?? "";

  return (
    <div
      className="prefs-page is-embedded terminal-settings-page"
      data-tone={tone}
    >
      <div className="prefs-category-content">
        <div className="prefs-category-stack terminal-settings-layout">
          <section className="prefs-card terminal-settings-section terminal-settings-section--mode">
            <div className="prefs-card-head">
              <div className="prefs-icon-badge" data-tone={tone} aria-hidden>
                <ShieldCheck size={20} />
              </div>
              <div>
                <h2 className="prefs-card-title">
                  {t("terminal.settings.mode.title")}
                </h2>
                <p className="prefs-card-sub">
                  {t("terminal.settings.mode.sub")}
                </p>
              </div>
            </div>
            <div className="terminal-mode-options">
              {(["system", "project"] as const).map((mode) => (
                <button
                  key={mode}
                  type="button"
                  className={`terminal-mode-option ${settings.executionMode === mode ? "is-active" : ""}`}
                  aria-pressed={settings.executionMode === mode}
                  onClick={() => update({ executionMode: mode })}
                >
                  <span>
                    {t(
                      mode === "system"
                        ? "terminal.settings.mode.system"
                        : "terminal.settings.mode.project",
                    )}
                  </span>
                  <small>
                    {t(
                      mode === "system"
                        ? "terminal.settings.mode.systemDesc"
                        : "terminal.settings.mode.projectDesc",
                    )}
                  </small>
                </button>
              ))}
            </div>
            <p className="terminal-settings-note">
              {t(
                settings.executionMode === "system"
                  ? "terminal.settings.mode.systemNote"
                  : "terminal.settings.mode.projectNote",
              )}
            </p>
          </section>

          <section className="prefs-card terminal-settings-section terminal-settings-section--font">
            <div className="prefs-card-head">
              <div className="prefs-icon-badge" data-tone={tone} aria-hidden>
                <Type size={20} />
              </div>
              <div>
                <h2 className="prefs-card-title">
                  {t("terminal.settings.font.title")}
                </h2>
                <p className="prefs-card-sub">
                  {t("terminal.settings.font.sub")}
                </p>
              </div>
            </div>

            <div className="terminal-settings-grid">
              <label className="terminal-settings-field terminal-settings-field--wide">
                <span>{t("terminal.settings.font.preset")}</span>
                <select
                  value={selectedPreset}
                  onChange={(event) => {
                    if (event.target.value)
                      update({ fontFamily: event.target.value });
                  }}
                >
                  <option value="">{t("terminal.settings.font.custom")}</option>
                  {TERMINAL_FONT_PRESETS.map((preset) => (
                    <option key={preset.label} value={preset.value}>
                      {preset.label}
                    </option>
                  ))}
                </select>
              </label>
              <label className="terminal-settings-field terminal-settings-field--wide">
                <span>{t("terminal.settings.font.family")}</span>
                <input
                  type="text"
                  value={settings.fontFamily}
                  spellCheck={false}
                  onChange={(event) =>
                    update({ fontFamily: event.target.value })
                  }
                />
              </label>
              <label className="terminal-settings-field">
                <span>{t("terminal.settings.font.size")}</span>
                <input
                  type="number"
                  min={9}
                  max={28}
                  step={0.5}
                  value={settings.fontSize}
                  onChange={(event) =>
                    update({ fontSize: Number(event.target.value) })
                  }
                />
              </label>
              <label className="terminal-settings-field">
                <span>{t("terminal.settings.font.lineHeight")}</span>
                <input
                  type="number"
                  min={1}
                  max={2}
                  step={0.05}
                  value={settings.lineHeight}
                  onChange={(event) =>
                    update({ lineHeight: Number(event.target.value) })
                  }
                />
              </label>
            </div>
          </section>

          <section className="prefs-card terminal-settings-section terminal-settings-section--behavior">
            <div className="prefs-card-head">
              <div className="prefs-icon-badge" data-tone={tone} aria-hidden>
                <TerminalSquare size={20} />
              </div>
              <div>
                <h2 className="prefs-card-title">
                  {t("terminal.settings.behavior.title")}
                </h2>
                <p className="prefs-card-sub">
                  {t("terminal.settings.behavior.sub")}
                </p>
              </div>
            </div>
            <div className="terminal-settings-grid">
              <label className="terminal-settings-field">
                <span>{t("terminal.settings.cursor.style")}</span>
                <select
                  value={settings.cursorStyle}
                  onChange={(event) =>
                    update({
                      cursorStyle: event.target
                        .value as TerminalSettings["cursorStyle"],
                    })
                  }
                >
                  <option value="bar">
                    {t("terminal.settings.cursor.bar")}
                  </option>
                  <option value="block">
                    {t("terminal.settings.cursor.block")}
                  </option>
                  <option value="underline">
                    {t("terminal.settings.cursor.underline")}
                  </option>
                </select>
              </label>
              <label className="terminal-settings-field">
                <span>{t("terminal.settings.scrollback")}</span>
                <input
                  type="number"
                  min={500}
                  max={50_000}
                  step={500}
                  value={settings.scrollback}
                  onChange={(event) =>
                    update({ scrollback: Number(event.target.value) })
                  }
                />
              </label>
              <div className="terminal-settings-toggle terminal-settings-field--wide">
                <div>
                  <strong>{t("terminal.settings.cursor.blink")}</strong>
                  <small>{t("terminal.settings.cursor.blinkDesc")}</small>
                </div>
                <button
                  type="button"
                  className="prefs-switch"
                  role="switch"
                  aria-checked={settings.cursorBlink}
                  aria-label={t("terminal.settings.cursor.blink")}
                  onClick={() => update({ cursorBlink: !settings.cursorBlink })}
                >
                  <span className="prefs-switch-thumb" />
                </button>
              </div>
            </div>
          </section>

          <aside className="terminal-settings-preview-pane">
            <div
              className="terminal-font-preview"
              style={{
                fontFamily: settings.fontFamily,
                fontSize: `${settings.fontSize}px`,
                lineHeight: settings.lineHeight,
              }}
              aria-label={t("terminal.settings.font.hint")}
            >
              <span>╭─  &nbsp; ~/astro/workspace</span>
              <span>╰─❯ git status</span>
            </div>
            <p className="terminal-settings-note">
              {t("terminal.settings.font.hint")}
            </p>
          </aside>

          <div className="terminal-settings-actions">
            <button
              type="button"
              className="prefs-diag-btn"
              onClick={() =>
                setSettings(saveTerminalSettings(DEFAULT_TERMINAL_SETTINGS))
              }
            >
              <RotateCcw size={14} aria-hidden />
              {t("terminal.settings.reset")}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
