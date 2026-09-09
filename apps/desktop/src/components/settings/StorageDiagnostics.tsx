import { useCallback, useEffect, useRef, useState } from "react";
import { HardDrive, RefreshCw } from "lucide-react";
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

export default function StorageDiagnostics({
  active,
  references = [],
  initialReport,
}: {
  active: boolean;
  references?: string[];
  initialReport?: StorageReport;
}) {
  const { locale } = useI18n();
  const copy = storageCopy[locale];
  const [report, setReport] = useState<StorageReport | null>(
    initialReport ?? null,
  );
  const [busy, setBusy] = useState(false);
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
          disabled={busy}
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
          <div aria-live="polite">
            <strong>{copy.states[report.state]}</strong>
            <p>{copy.guidance[report.state]}</p>
          </div>
          <dl className="storage-diagnostics-paths">
            <dt>{copy.root}</dt>
            <dd>
              <code>{report.rootPath}</code>
            </dd>
            <dt>{copy.config}</dt>
            <dd>
              <code>{report.configPath}</code>
              {!report.configPresent && <span> · {copy.missing}</span>}
            </dd>
            <dt>{copy.version}</dt>
            <dd>{report.settingsVersion ?? copy.defaults}</dd>
          </dl>
          {report.partial && (
            <p className="storage-diagnostics-warning">{copy.partial}</p>
          )}
          {totals.skippedLinks > 0 && (
            <p>
              {totals.skippedLinks} {copy.links}
            </p>
          )}
          {report.issues.length > 0 && (
            <details open>
              <summary>
                {copy.issues} · {report.issues.length}
              </summary>
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
            </details>
          )}
          <details open>
            <summary>
              {copy.domains} · {copy.total} {storageBytes(totals.bytes)}
            </summary>
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
          </details>
          <details>
            <summary>
              {copy.preview} · {storageBytes(totals.previewBytes)} /{" "}
              {totals.previewFiles} {copy.candidates}
            </summary>
            <p>{copy.sample}</p>
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
            <ul>
              {[copy.cache, copy.logs, copy.backups, copy.protected].map(
                (text) => (
                  <li key={text}>{text}</li>
                ),
              )}
            </ul>
          </details>
          <p className="prefs-card-sub">{copy.references}</p>
        </div>
      )}
    </section>
  );
}
