import { invoke } from "@tauri-apps/api/core";
import {
  Download,
  Globe2,
  Monitor,
  ShieldCheck,
  Trash2,
  Wifi,
} from "lucide-react";
import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type FormEvent,
} from "react";
import { useI18n } from "../../i18n/LocaleContext";
import { normalizeBrowserUrl } from "../../lib/browser/browserUrl";

type BrowserApprovalRule = {
  origin: string;
  actionClass: string;
};

type BrowserSettingsState = {
  homePage: string;
  viewportWidth: number;
  viewportHeight: number;
  allowLoopback: boolean;
  downloadsEnabled: boolean;
  browserAvailable: boolean;
  dataDirectory: string;
  approvalRules: BrowserApprovalRule[];
};

type BrowserRuntimeSettings = Pick<
  BrowserSettingsState,
  | "homePage"
  | "viewportWidth"
  | "viewportHeight"
  | "allowLoopback"
  | "downloadsEnabled"
>;

type Props = {
  active?: boolean;
  tone?: string;
};

const FALLBACK_SETTINGS: BrowserSettingsState = {
  homePage: "https://example.com/",
  viewportWidth: 1280,
  viewportHeight: 800,
  allowLoopback: true,
  downloadsEnabled: true,
  browserAvailable: false,
  dataDirectory: "~/.astro/browser",
  approvalRules: [],
};

const VIEWPORT_PRESETS = [
  { width: 1280, height: 800, label: "1280 × 800" },
  { width: 1440, height: 900, label: "1440 × 900" },
  { width: 390, height: 844, label: "390 × 844" },
] as const;

function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function runtimeSettings(state: BrowserSettingsState): BrowserRuntimeSettings {
  return {
    homePage: state.homePage,
    viewportWidth: state.viewportWidth,
    viewportHeight: state.viewportHeight,
    allowLoopback: state.allowLoopback,
    downloadsEnabled: state.downloadsEnabled,
  };
}

export default function BrowserSettingsPanel({
  active = true,
  tone = "twilight",
}: Props) {
  const { t } = useI18n();
  const [settings, setSettings] =
    useState<BrowserSettingsState>(FALLBACK_SETTINGS);
  const [homePage, setHomePage] = useState(FALLBACK_SETTINGS.homePage);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const savingRef = useRef(false);
  const [error, setError] = useState("");

  const refresh = useCallback(async () => {
    if (!isTauri()) {
      setLoading(false);
      return;
    }
    setError("");
    try {
      const next = await invoke<BrowserSettingsState>("browser_get_settings");
      setSettings(next);
      setHomePage(next.homePage);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (active) void refresh();
  }, [active, refresh]);

  const save = async (
    next: BrowserRuntimeSettings,
    options: { syncHomePage?: boolean } = {},
  ) => {
    if (!isTauri() || savingRef.current) return;
    savingRef.current = true;
    setSaving(true);
    setError("");
    try {
      const saved = await invoke<BrowserSettingsState>("browser_set_settings", {
        settings: next,
      });
      setSettings(saved);
      if (options.syncHomePage) setHomePage(saved.homePage);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      savingRef.current = false;
      setSaving(false);
    }
  };

  const normalizedHomePage = normalizeBrowserUrl(homePage);

  const submitHomePage = (event: FormEvent) => {
    event.preventDefault();
    if (!normalizedHomePage || normalizedHomePage === settings.homePage) return;
    void save(
      { ...runtimeSettings(settings), homePage: normalizedHomePage },
      { syncHomePage: true },
    );
  };

  const revokeApproval = async (rule: BrowserApprovalRule) => {
    if (!isTauri() || savingRef.current) return;
    savingRef.current = true;
    setSaving(true);
    setError("");
    try {
      const next = await invoke<BrowserSettingsState>(
        "browser_revoke_approval",
        { origin: rule.origin, actionClass: rule.actionClass },
      );
      setSettings(next);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      savingRef.current = false;
      setSaving(false);
    }
  };

  const selectedViewport = `${settings.viewportWidth}x${settings.viewportHeight}`;

  return (
    <div
      className="prefs-page is-embedded browser-settings-page"
      data-tone={tone}
    >
      <div className="prefs-category-content">
        <div className="prefs-category-stack browser-settings-layout">
          <section className="prefs-card browser-settings-status-card browser-settings-card--runtime">
            <div className="prefs-card-head">
              <div className="prefs-icon-badge" data-tone={tone} aria-hidden>
                <Globe2 size={21} />
              </div>
              <div>
                <h2 className="prefs-card-title">
                  {t("browser.settings.runtime.title")}
                </h2>
                <p className="prefs-card-sub">
                  {t("browser.settings.runtime.sub")}
                </p>
              </div>
              <span
                className={`browser-runtime-status ${settings.browserAvailable ? "is-ready" : "is-missing"}`}
              >
                <span aria-hidden />
                {loading
                  ? t("browser.settings.status.checking")
                  : settings.browserAvailable
                    ? t("browser.settings.status.ready")
                    : t("browser.settings.status.missing")}
              </span>
            </div>
            <div className="browser-data-path">
              <span className="browser-setting-copy">
                <strong>{t("browser.settings.data.label")}</strong>
                <small>{t("browser.settings.data.desc")}</small>
              </span>
              <code>{settings.dataDirectory}</code>
            </div>
          </section>

          <section className="prefs-card browser-settings-card--startup">
            <div className="prefs-card-head">
              <div className="prefs-icon-badge" data-tone={tone} aria-hidden>
                <Monitor size={20} />
              </div>
              <div>
                <h2 className="prefs-card-title">
                  {t("browser.settings.startup.title")}
                </h2>
                <p className="prefs-card-sub">
                  {t("browser.settings.startup.sub")}
                </p>
              </div>
            </div>

            <form className="browser-settings-home" onSubmit={submitHomePage}>
              <label
                className="browser-setting-copy"
                htmlFor="browser-home-page"
              >
                <strong>{t("browser.settings.home.label")}</strong>
                <small>{t("browser.settings.home.desc")}</small>
              </label>
              <div>
                <input
                  id="browser-home-page"
                  className="prefs-diag-input"
                  value={homePage}
                  onChange={(event) => setHomePage(event.target.value)}
                  placeholder="https://example.com"
                  spellCheck={false}
                  disabled={saving}
                />
                <button
                  type="submit"
                  className="prefs-diag-btn primary"
                  disabled={
                    saving ||
                    !normalizedHomePage ||
                    normalizedHomePage === settings.homePage
                  }
                >
                  {saving
                    ? t("browser.settings.saving")
                    : t("browser.settings.save")}
                </button>
              </div>
            </form>

            <div className="browser-viewport-setting">
              <span className="browser-setting-copy">
                <strong>{t("browser.settings.viewport.label")}</strong>
                <small>{t("browser.settings.viewport.hint")}</small>
              </span>
              <div
                className="browser-viewport-options"
                role="radiogroup"
                aria-label={t("browser.settings.viewport.label")}
              >
                {VIEWPORT_PRESETS.map((preset) => {
                  const value = `${preset.width}x${preset.height}`;
                  return (
                    <button
                      key={value}
                      type="button"
                      role="radio"
                      aria-checked={selectedViewport === value}
                      className={
                        selectedViewport === value ? "is-active" : undefined
                      }
                      disabled={saving}
                      onClick={() =>
                        void save({
                          ...runtimeSettings(settings),
                          viewportWidth: preset.width,
                          viewportHeight: preset.height,
                        })
                      }
                    >
                      {preset.label}
                    </button>
                  );
                })}
              </div>
            </div>
          </section>

          <section className="prefs-card browser-settings-card--permissions">
            <div className="prefs-card-head">
              <div className="prefs-icon-badge" data-tone={tone} aria-hidden>
                <ShieldCheck size={20} />
              </div>
              <div>
                <h2 className="prefs-card-title">
                  {t("browser.settings.permissions.title")}
                </h2>
                <p className="prefs-card-sub">
                  {t("browser.settings.permissions.sub")}
                </p>
              </div>
            </div>

            <div className="prefs-toggle-list browser-permission-toggles">
              <div className="prefs-toggle-row">
                <span className="prefs-toggle-icon" aria-hidden>
                  <Wifi size={17} />
                </span>
                <span className="prefs-toggle-text">
                  <span className="prefs-toggle-label">
                    {t("browser.settings.loopback.label")}
                  </span>
                  <span className="prefs-toggle-desc">
                    {t("browser.settings.loopback.desc")}
                  </span>
                </span>
                <button
                  type="button"
                  role="switch"
                  className="prefs-switch"
                  aria-checked={settings.allowLoopback}
                  aria-label={t("browser.settings.loopback.label")}
                  disabled={saving}
                  onClick={() =>
                    void save({
                      ...runtimeSettings(settings),
                      allowLoopback: !settings.allowLoopback,
                    })
                  }
                >
                  <span className="prefs-switch-thumb" />
                </button>
              </div>

              <div className="prefs-toggle-row">
                <span className="prefs-toggle-icon" aria-hidden>
                  <Download size={17} />
                </span>
                <span className="prefs-toggle-text">
                  <span className="prefs-toggle-label">
                    {t("browser.settings.downloads.label")}
                  </span>
                  <span className="prefs-toggle-desc">
                    {t("browser.settings.downloads.desc")}
                  </span>
                </span>
                <button
                  type="button"
                  role="switch"
                  className="prefs-switch"
                  aria-checked={settings.downloadsEnabled}
                  aria-label={t("browser.settings.downloads.label")}
                  disabled={saving}
                  onClick={() =>
                    void save({
                      ...runtimeSettings(settings),
                      downloadsEnabled: !settings.downloadsEnabled,
                    })
                  }
                >
                  <span className="prefs-switch-thumb" />
                </button>
              </div>
            </div>
          </section>

          <section className="prefs-card browser-settings-card--sites">
            <div className="prefs-card-head">
              <div className="prefs-icon-badge" data-tone={tone} aria-hidden>
                <ShieldCheck size={20} />
              </div>
              <div>
                <h2 className="prefs-card-title">
                  {t("browser.settings.sitePermissions.title")}
                </h2>
                <p className="prefs-card-sub">
                  {t("browser.settings.sitePermissions.sub")}
                </p>
              </div>
            </div>
            {settings.approvalRules.length === 0 ? (
              <p className="browser-permissions-empty">
                {t("approvals.browser.empty")}
              </p>
            ) : (
              <ul className="browser-permissions-list">
                {settings.approvalRules.map((rule) => (
                  <li key={`${rule.origin}:${rule.actionClass}`}>
                    <span className="browser-permission-site-icon" aria-hidden>
                      <Globe2 size={15} />
                    </span>
                    <span className="browser-permission-site-copy">
                      <code>{rule.origin}</code>
                      <small>
                        {t("browser.settings.sitePermissions.approved")}
                      </small>
                    </span>
                    <span className="browser-permission-kind">
                      {rule.actionClass === "state_changing"
                        ? t("approvals.browser.stateChanging")
                        : rule.actionClass}
                    </span>
                    <button
                      type="button"
                      disabled={saving}
                      onClick={() => void revokeApproval(rule)}
                    >
                      <Trash2 size={13} aria-hidden />
                      {t("browser.settings.revoke")}
                    </button>
                  </li>
                ))}
              </ul>
            )}
            <p className="browser-sensitive-note">
              {t("browser.settings.sensitive")}
            </p>
          </section>

          {error ? (
            <p className="prefs-diag-error" role="alert">
              {error}
            </p>
          ) : null}
        </div>
      </div>
    </div>
  );
}
