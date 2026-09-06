import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  Check,
  CheckCircle2,
  CircleAlert,
  Copy,
  Download,
  ExternalLink,
  LoaderCircle,
  MapPin,
  PackageCheck,
  RefreshCw,
  TerminalSquare,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";

import { useI18n } from "../../i18n/LocaleContext";
import type { MessageKey } from "../../i18n/messages";

type EnvironmentDependency = {
  id: string;
  name: string;
  binary: string;
  installed: boolean;
  version: string | null;
  path: string | null;
  installPath: string;
  installCommand: string | null;
  canInstall: boolean;
  installUnavailableReason: string | null;
};

type InstallResult = {
  dependency: EnvironmentDependency;
  output: string;
};

type DependencyMeta = {
  descriptionKey: MessageKey;
  officialUrl: string;
};

const DEPENDENCY_META: Record<string, DependencyMeta> = {
  uv: {
    descriptionKey: "environmentDependencies.item.uv",
    officialUrl: "https://docs.astral.sh/uv/getting-started/installation/",
  },
  rtk: {
    descriptionKey: "environmentDependencies.item.rtk",
    officialUrl: "https://www.rtk-ai.app/",
  },
  fd: {
    descriptionKey: "environmentDependencies.item.fd",
    officialUrl: "https://github.com/sharkdp/fd#installation",
  },
  ripgrep: {
    descriptionKey: "environmentDependencies.item.ripgrep",
    officialUrl: "https://github.com/BurntSushi/ripgrep#installation",
  },
  bun: {
    descriptionKey: "environmentDependencies.item.bun",
    officialUrl: "https://bun.sh/docs/installation",
  },
  "lark-cli": {
    descriptionKey: "environmentDependencies.item.larkCli",
    officialUrl: "https://github.com/larksuite/cli#readme",
  },
};

type Props = {
  active?: boolean;
  tone?: string;
};

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export default function EnvironmentDependenciesPanel({
  active = true,
  tone = "twilight",
}: Props) {
  const { t } = useI18n();
  const [dependencies, setDependencies] = useState<EnvironmentDependency[]>([]);
  const [loading, setLoading] = useState(false);
  const [installingId, setInstallingId] = useState<string | null>(null);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  const refresh = useCallback(async () => {
    setLoading(true);
    setError("");
    try {
      setDependencies(
        await invoke<EnvironmentDependency[]>("list_environment_dependencies"),
      );
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (!active) return;
    void refresh();
  }, [active, refresh]);

  const installedCount = useMemo(
    () => dependencies.filter((dependency) => dependency.installed).length,
    [dependencies],
  );

  const install = useCallback(
    async (dependency: EnvironmentDependency) => {
      setInstallingId(dependency.id);
      setError("");
      setNotice("");
      try {
        const result = await invoke<InstallResult>(
          "install_environment_dependency",
          { dependencyId: dependency.id },
        );
        setDependencies((current) =>
          current.map((item) =>
            item.id === dependency.id ? result.dependency : item,
          ),
        );
        setNotice(
          result.dependency.installed
            ? t("environmentDependencies.install.success", {
                name: dependency.name,
              })
            : t("environmentDependencies.install.restart", {
                name: dependency.name,
              }),
        );
      } catch (reason) {
        setError(
          t("environmentDependencies.install.failed", {
            name: dependency.name,
            error: errorMessage(reason),
          }),
        );
      } finally {
        setInstallingId(null);
      }
    },
    [t],
  );

  const copyCommand = useCallback(async (dependency: EnvironmentDependency) => {
    if (!dependency.installCommand) return;
    try {
      await navigator.clipboard.writeText(dependency.installCommand);
      setCopiedId(dependency.id);
      window.setTimeout(() => setCopiedId(null), 1_600);
    } catch {
      setCopiedId(null);
    }
  }, []);

  const unavailableReason = useCallback(
    (reason: string) =>
      t(
        reason === "npm_required"
          ? "environmentDependencies.unavailable.npm"
          : "environmentDependencies.unavailable.installer",
      ),
    [t],
  );

  return (
    <div
      className="prefs-page is-embedded environment-dependencies-page"
      data-tone={tone}
    >
      <div className="prefs-category-content">
        <div className="prefs-category-stack environment-dependencies-layout">
          <section className="prefs-card environment-dependencies-overview">
            <div className="prefs-card-head">
              <div className="prefs-icon-badge" data-tone={tone} aria-hidden>
                <PackageCheck size={20} />
              </div>
              <div>
                <h2 className="prefs-card-title">
                  {t("environmentDependencies.title")}
                </h2>
                <p className="prefs-card-sub">
                  {t("environmentDependencies.subtitle")}
                </p>
              </div>
              <div
                className="environment-dependencies-summary"
                aria-live="polite"
              >
                <strong>{installedCount}</strong>
                <span>/ {dependencies.length || 6}</span>
                <small>{t("environmentDependencies.ready")}</small>
              </div>
              <button
                type="button"
                className="prefs-diag-btn environment-dependencies-refresh"
                onClick={() => void refresh()}
                disabled={loading || installingId !== null}
              >
                <RefreshCw size={14} className={loading ? "is-spinning" : ""} />
                {t("environmentDependencies.rescan")}
              </button>
            </div>
            <p className="environment-dependencies-note">
              {t("environmentDependencies.note")}
            </p>
          </section>

          {notice ? (
            <p
              className="environment-dependencies-feedback is-success"
              role="status"
            >
              {notice}
            </p>
          ) : null}
          {error ? (
            <p
              className="environment-dependencies-feedback is-error"
              role="alert"
            >
              {error}
            </p>
          ) : null}

          <section
            className="environment-dependencies-list"
            aria-busy={loading}
            aria-label={t("environmentDependencies.title")}
            role="list"
          >
            {loading && dependencies.length === 0 ? (
              <div className="prefs-card environment-dependencies-loading">
                <LoaderCircle size={18} className="is-spinning" aria-hidden />
                {t("environmentDependencies.scanning")}
              </div>
            ) : null}

            {dependencies.map((dependency) => {
              const meta = DEPENDENCY_META[dependency.id];
              const isInstalling = installingId === dependency.id;
              return (
                <article
                  key={dependency.id}
                  className="prefs-card environment-dependency-card"
                  data-installed={dependency.installed}
                  role="listitem"
                >
                  <div className="environment-dependency-status" aria-hidden>
                    {dependency.installed ? (
                      <CheckCircle2 size={20} />
                    ) : (
                      <CircleAlert size={20} />
                    )}
                  </div>

                  <div className="environment-dependency-main">
                    <div className="environment-dependency-title-row">
                      <h3>{dependency.name}</h3>
                      <code>{dependency.binary}</code>
                      <span
                        className={`environment-dependency-badge ${dependency.installed ? "is-ready" : "is-missing"}`}
                      >
                        {t(
                          dependency.installed
                            ? "environmentDependencies.installed"
                            : "environmentDependencies.missing",
                        )}
                      </span>
                    </div>
                    <p>
                      {meta
                        ? t(meta.descriptionKey)
                        : t("environmentDependencies.item.fallback")}
                    </p>
                    <dl className="environment-dependency-details">
                      <div>
                        <dt>
                          <TerminalSquare size={13} aria-hidden />
                          {t("environmentDependencies.version")}
                        </dt>
                        <dd>{dependency.version ?? "—"}</dd>
                      </div>
                      <div>
                        <dt>
                          <MapPin size={13} aria-hidden />
                          {t(
                            dependency.installed
                              ? "environmentDependencies.detectedPath"
                              : "environmentDependencies.installPath",
                          )}
                        </dt>
                        <dd title={dependency.path ?? dependency.installPath}>
                          {dependency.path ?? dependency.installPath}
                        </dd>
                      </div>
                    </dl>
                    {dependency.installCommand ? (
                      <div className="environment-dependency-command">
                        <code>{dependency.installCommand}</code>
                        <button
                          type="button"
                          onClick={() => void copyCommand(dependency)}
                          aria-label={t("environmentDependencies.copyCommand")}
                          title={t("environmentDependencies.copyCommand")}
                        >
                          {copiedId === dependency.id ? (
                            <Check size={14} />
                          ) : (
                            <Copy size={14} />
                          )}
                        </button>
                      </div>
                    ) : null}
                    {!dependency.canInstall &&
                    dependency.installUnavailableReason ? (
                      <small className="environment-dependency-unavailable">
                        {unavailableReason(dependency.installUnavailableReason)}
                      </small>
                    ) : null}
                  </div>

                  <div className="environment-dependency-actions">
                    {meta ? (
                      <button
                        type="button"
                        className="prefs-diag-btn"
                        onClick={() => void openUrl(meta.officialUrl)}
                      >
                        <ExternalLink size={14} />
                        {t("environmentDependencies.official")}
                      </button>
                    ) : null}
                    <button
                      type="button"
                      className="prefs-diag-btn primary"
                      disabled={
                        dependency.installed ||
                        !dependency.canInstall ||
                        installingId !== null
                      }
                      onClick={() => void install(dependency)}
                    >
                      {isInstalling ? (
                        <LoaderCircle size={14} className="is-spinning" />
                      ) : dependency.installed ? (
                        <Check size={14} />
                      ) : (
                        <Download size={14} />
                      )}
                      {t(
                        isInstalling
                          ? "environmentDependencies.installing"
                          : dependency.installed
                            ? "environmentDependencies.installed"
                            : "environmentDependencies.install",
                      )}
                    </button>
                  </div>
                </article>
              );
            })}
          </section>
        </div>
      </div>
    </div>
  );
}
