import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "../../i18n/LocaleContext";
import AppDialog from "../ui/AppDialog";
import "../../styles/features/settings/storage-diagnostics.css";
import { storageBytes } from "../../lib/settings/storageDiagnostics";
import {
  canConfirmCleanup,
  cleanupCopy,
  cleanupToken,
  parseCleanupPlan,
  parseCleanupResult,
  type CleanupPlan,
  type CleanupResult,
} from "../../lib/settings/storageCleanup";

export default function StorageCleanup({
  active = true,
  enabled,
  rootPath,
  onChanged,
  onBusyChange,
}: {
  active?: boolean;
  enabled: boolean;
  rootPath: string;
  onChanged: () => void;
  onBusyChange?: (busy: boolean) => void;
}) {
  const { locale } = useI18n();
  const copy = cleanupCopy[locale];
  const [plan, setPlan] = useState<CleanupPlan | null>(null);
  const [acknowledged, setAcknowledged] = useState(false);
  const [phase, setPhase] = useState<"idle" | "preparing" | "moving">("idle");
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<CleanupResult | null>(null);
  const generation = useRef(0),
    token = useRef<string | null>(null),
    acting = useRef(false);
  const acknowledgedRef = useRef(false);
  const activeRef = useRef(active);
  activeRef.current = active;
  const acknowledge = (value: boolean) => {
    acknowledgedRef.current = value;
    setAcknowledged(value);
  };
  const discard = useCallback((value: string | null) => {
    if (value)
      void invoke("discard_storage_cleanup", { token: value }).catch(() => {});
  }, []);
  useEffect(() => {
    setPlan(null);
    acknowledgedRef.current = false;
    setAcknowledged(false);
    setResult(null);
    setError(null);
    setPhase("idle");
    acting.current = false;
    return () => {
      generation.current += 1;
      discard(token.current);
      token.current = null;
      onBusyChange?.(false);
    };
  }, [active, rootPath, discard, onBusyChange]);
  const fail = (value: unknown) =>
    setError(
      typeof value === "string"
        ? value
        : value instanceof Error
          ? value.message
          : "cleanup_unavailable",
    );
  const cancel = () => {
    discard(token.current);
    token.current = null;
    setPlan(null);
    acknowledge(false);
  };
  const prepare = async () => {
    if (!active || !enabled || acting.current) return;
    acting.current = true;
    const request = ++generation.current;
    setPhase("preparing");
    onBusyChange?.(true);
    setError(null);
    setResult(null);
    let raw: unknown;
    try {
      raw = await invoke("prepare_storage_cleanup");
      const next = parseCleanupPlan(raw);
      if (generation.current !== request) {
        discard(next.token);
        return;
      }
      token.current = next.token;
      acknowledge(false);
      setPlan(next);
    } catch (value) {
      if (
        typeof raw === "object" &&
        raw !== null &&
        "token" in raw &&
        cleanupToken(raw.token)
      )
        discard(raw.token);
      if (generation.current === request) fail(value);
    } finally {
      if (generation.current === request) {
        acting.current = false;
        setPhase("idle");
        onBusyChange?.(false);
      }
    }
  };
  const confirm = async () => {
    if (!activeRef.current || !plan || token.current !== plan.token) return;
    if (!canConfirmCleanup(plan, acknowledgedRef.current, acting.current)) {
      if (plan && Date.now() >= plan.expiresAtMs) {
        cancel();
        setError("cleanup_expired");
      }
      return;
    }
    const current = plan!;
    const request = ++generation.current;
    acting.current = true;
    token.current = null;
    setPlan(null);
    setPhase("moving");
    onBusyChange?.(true);
    setError(null);
    try {
      const response = parseCleanupResult(
        await invoke("execute_storage_cleanup", {
          token: current.token,
          confirmed: true,
        }),
      );
      if (generation.current === request) {
        setResult(response);
        onChanged();
      }
    } catch (value) {
      if (generation.current === request) {
        fail(value);
        onChanged();
      }
    } finally {
      if (generation.current === request) {
        acting.current = false;
        setPhase("idle");
        onBusyChange?.(false);
      }
    }
  };
  const reveal = async () => {
    if (!result) return;
    try {
      await invoke("reveal_storage_recovery", { batchId: result.batchId });
    } catch (value) {
      fail(value);
    }
  };
  const complete =
    result?.manifestComplete &&
    result.unverifiedFiles === 0 &&
    result.movedFiles === result.outcomes.length;
  return (
    <div className="storage-cleanup-controls">
      <button
        type="button"
        className="prefs-diag-btn"
        disabled={!enabled || phase !== "idle" || !!plan}
        onClick={() => void prepare()}
      >
        {phase === "preparing"
          ? copy.preparing
          : phase === "moving"
            ? copy.moving
            : copy.prepare}
      </button>
      <p className="prefs-card-sub">{copy.explanation}</p>
      {error && (
        <p role="alert" className="storage-diagnostics-warning">
          {copy.errors[error as keyof typeof copy.errors] ??
            copy.errors.cleanup_unavailable}
        </p>
      )}
      {result && (
        <div className="storage-cleanup-result" role="status">
          <strong>{complete ? copy.complete : copy.partial}</strong>
          <p>
            {copy.verified}: {result.movedFiles} {copy.files} ·{" "}
            {storageBytes(result.movedBytes)}
          </p>
          {result.unverifiedFiles > 0 && (
            <p>
              {copy.unverified}: {result.unverifiedFiles}
            </p>
          )}
          {!result.manifestComplete && (
            <p className="storage-diagnostics-warning">
              {copy.manifestWarning}
            </p>
          )}
          <code>{result.recoveryPath}</code>
          <p>{copy.recover}</p>
          <button
            type="button"
            className="prefs-diag-btn"
            onClick={() => void reveal()}
          >
            {copy.reveal}
          </button>
          {!complete && (
            <ul>
              {result.outcomes.map((item) => (
                <li key={item.path}>
                  <code>{item.path}</code>
                  <span>
                    {copy.statuses[item.status as keyof typeof copy.statuses]}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
      <AppDialog
        open={!!plan}
        title={copy.title}
        message={copy.explanation}
        variant="danger"
        confirmOnEnter={false}
        trapFocus
        confirmLabel={copy.confirm}
        cancelLabel={copy.cancel}
        confirmDisabled={
          !canConfirmCleanup(plan, acknowledged, phase !== "idle")
        }
        onConfirm={() => void confirm()}
        onCancel={cancel}
      >
        {plan && (
          <div className="storage-cleanup-confirmation">
            <p>
              {copy.root}: <code>{plan.rootPath}</code>
            </p>
            <strong>
              {plan.items.length} {copy.files} · {storageBytes(plan.totalBytes)}
            </strong>
            <ul className="storage-cleanup-list">
              {plan.items.map((item) => (
                <li key={item.path}>
                  <code>{item.path}</code>
                  <span>{storageBytes(item.bytes)}</span>
                </li>
              ))}
            </ul>
            <p>{copy.limits}</p>
            {plan.omittedFiles && <p>{copy.omitted}</p>}
            <label className="storage-cleanup-ack">
              <input
                type="checkbox"
                checked={acknowledged}
                onChange={(event) => acknowledge(event.target.checked)}
              />
              {copy.acknowledge}
            </label>
          </div>
        )}
      </AppDialog>
    </div>
  );
}
