import { useCallback, useEffect, useRef, useState } from "react";
import { Check, Copy, HardDrive, RefreshCw } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "../../i18n/LocaleContext";
import {
  parseStorageReport,
  storageBytes,
  storageCopy,
  storageTotals,
  type StorageReport,
} from "../../lib/settings/storageDiagnostics";
import "../../styles/features/settings/storage-diagnostics.css";
import StorageCleanup from "./StorageCleanup";

function StoragePath({ value }: { value: string }) {
  const { t } = useI18n();
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState(false);
  useEffect(() => {
    setCopied(false);
    setError(false);
  }, [value]);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(value);
      setCopied(true);
      setError(false);
    } catch {
      setError(true);
    }
  };
  return (
    <div className="storage-diagnostics-path-value">
      <code title={value}>{value}</code>
      <button
        type="button"
        className="diagnostics-icon-button"
        aria-label={t(copied ? "prefs.diag.copied" : "prefs.diag.copyPath")}
        onClick={() => void copy()}
      >
        {copied ? <Check size={14} /> : <Copy size={14} />}
      </button>
      {error && <span role="alert">{t("prefs.diag.copyFailed")}</span>}
    </div>
  );
}

export default function StorageDiagnostics({
  active,
  references = [],
  initialReport,
  onBusyChange,
}: {
  active: boolean;
  references?: string[];
  initialReport?: StorageReport;
  onBusyChange?: (busy: boolean) => void;
}) {
  const { locale } = useI18n();
  const copy = storageCopy[locale];
  const [report, setReport] = useState<StorageReport | null>(
    initialReport ?? null,
  );
  const [busy, setBusy] = useState(false);
  const [cleanupBusy, setCleanupBusy] = useState(false);
  const changeCleanupBusy = useCallback(
    (value: boolean) => {
      setCleanupBusy(value);
      onBusyChange?.(value);
    },
    [onBusyChange],
  );
  const [error, setError] = useState(false);
  const requestId = useRef(0);
  const referenceKey = JSON.stringify([...new Set(references)].slice(0, 100));
  const refresh = useCallback(async () => {
    const id = ++requestId.current;
    setBusy(true);
    setError(false);
    try {
      const value = await invoke("inspect_home_storage", {
        referencePaths: JSON.parse(referenceKey) as string[],
      });
      if (id === requestId.current) setReport(parseStorageReport(value));
    } catch {
      if (id === requestId.current) setError(true);
    } finally {
      if (id === requestId.current) setBusy(false);
    }
  }, [referenceKey]);
  useEffect(() => {
    if (active && !initialReport) void refresh();
    return () => {
      requestId.current += 1;
    };
  }, [active, initialReport, refresh]);
  const totals = report ? storageTotals(report) : null;
  return (
    <section
      className="prefs-card storage-diagnostics"
      aria-labelledby="storage-diagnostics-title"
      aria-busy={busy}
    >
      <div className="prefs-card-head">
        <HardDrive size={22} aria-hidden />
        <div className="prefs-diag-card-heading">
          <h2 id="storage-diagnostics-title" className="prefs-card-title">
            {copy.title}
          </h2>
          <p className="prefs-card-sub">{copy.sub}</p>
        </div>
        <button
          type="button"
          className="prefs-diag-btn"
          disabled={busy || cleanupBusy}
          onClick={() => void refresh()}
        >
          <RefreshCw size={13} aria-hidden />
          {busy ? copy.loading : copy.refresh}
        </button>
      </div>
      {error && (
        <p role="alert" className="storage-diagnostics-warning">
          {copy.error}
        </p>
      )}
      {report && totals && (
        <div className="storage-diagnostics-body">
          <div className="storage-diagnostics-summary" aria-live="polite">
            <div>
              <span>{copy.issues}</span>
              <strong>{copy.states[report.state]}</strong>
              <small>
                {report.issues.length} {copy.findingsCount}
              </small>
            </div>
            <div>
              <span>{copy.total}</span>
              <strong>{storageBytes(totals.bytes)}</strong>
              <small>{report.partial ? copy.partialLabel : copy.scanned}</small>
            </div>
            <div>
              <span>{copy.preview}</span>
              <strong>{storageBytes(totals.previewBytes)}</strong>
              <small>
                {totals.previewFiles} {copy.candidates}
              </small>
            </div>
          </div>
          <p className="storage-diagnostics-guidance">
            {copy.guidance[report.state]}
          </p>
          <div className="storage-diagnostics-columns">
            <section className="storage-diagnostics-section">
              <h3>{copy.configuration}</h3>
              <dl className="storage-diagnostics-paths">
                <dt>{copy.root}</dt>
                <dd>
                  <StoragePath value={report.rootPath} />
                </dd>
                <dt>{copy.config}</dt>
                <dd>
                  <StoragePath value={report.configPath} />
                  {!report.configPresent && <span>{copy.missing}</span>}
                </dd>
                <dt>{copy.version}</dt>
                <dd>{report.settingsVersion ?? copy.defaults}</dd>
              </dl>
              {report.issues.length > 0 && (
                <div className="storage-diagnostics-findings">
                  <h4>
                    {copy.issues} · {report.issues.length}
                  </h4>
                  <ul>
                    {report.issues.map((issue, index) => (
                      <li key={`${issue.code}:${index}`}>
                        <span>
                          {copy.issueNames[
                            issue.code as keyof typeof copy.issueNames
                          ] ?? issue.code}
                        </span>
                        {issue.path && <code>{issue.path}</code>}
                      </li>
                    ))}
                  </ul>
                </div>
              )}
            </section>
            <section className="storage-diagnostics-section">
              <h3>{copy.domains}</h3>
              {report.partial && (
                <p className="storage-diagnostics-warning">{copy.partial}</p>
              )}
              {totals.skippedLinks > 0 && (
                <p>
                  {totals.skippedLinks} {copy.links}
                </p>
              )}
              <dl className="storage-diagnostics-domains">
                {report.domains.map((domain) => (
                  <div key={domain.id}>
                    <dt>
                      {copy.domainNames[
                        domain.id as keyof typeof copy.domainNames
                      ] ?? domain.id}
                    </dt>
                    <dd>
                      <span>{storageBytes(domain.bytes)}</span>
                      <small>
                        {domain.files} {copy.files}
                      </small>
                    </dd>
                  </div>
                ))}
              </dl>
              <p className="prefs-card-sub">{copy.references}</p>
            </section>
          </div>
          <section className="storage-diagnostics-section storage-diagnostics-maintenance">
            <h3>{copy.maintenance}</h3>
            <details>
              <summary>
                {copy.preview} · {storageBytes(totals.previewBytes)} /{" "}
                {totals.previewFiles} {copy.candidates}
              </summary>
              {report.previewPartial && (
                <p className="storage-diagnostics-warning">
                  {copy.previewPartial}
                </p>
              )}
              <p>{copy.sample}</p>
              <StorageCleanup
                active={active}
                enabled={
                  report.state === "ready" &&
                  (totals.previewFiles > 0 ||
                    report.partial ||
                    report.previewPartial) &&
                  !busy
                }
                rootPath={report.rootPath}
                onChanged={() => void refresh()}
                onBusyChange={changeCleanupBusy}
              />
              {report.cleanupPreview.length ? (
                <ul>
                  {report.cleanupPreview.map((item) => (
                    <li key={item.path}>
                      <code>{item.path}</code>
                      <span>{storageBytes(item.bytes)}</span>
                    </li>
                  ))}
                </ul>
              ) : (
                <p>{copy.empty}</p>
              )}
            </details>
            <details>
              <summary>{copy.policies}</summary>
              {report.cachePolicies.map((policy) => (
                <div
                  className="storage-diagnostics-cache-policy"
                  key={policy.domain}
                >
                  <strong>
                    {copy.domainNames[
                      policy.domain as keyof typeof copy.domainNames
                    ] ?? policy.domain}{" "}
                    · {policy.enabled ? copy.enabled : copy.disabled}
                  </strong>
                  <code>{policy.directory}</code>
                  <span>
                    {copy.ttl}: {policy.ttlSeconds} · {copy.capacity}:{" "}
                    {policy.maxSizeMb} MiB
                  </span>
                  {policy.status !== "in_home" && (
                    <span>
                      {copy.policyStates[
                        policy.status as keyof typeof copy.policyStates
                      ] ?? policy.status}
                    </span>
                  )}
                </div>
              ))}
              <ul>
                {[copy.cache, copy.logs, copy.backups, copy.protected].map(
                  (text) => (
                    <li key={text}>{text}</li>
                  ),
                )}
              </ul>
            </details>
          </section>
        </div>
      )}
    </section>
  );
}
