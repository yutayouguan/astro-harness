import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

import {
  diagnosticLogTimeBounds,
  toLocalDateTimeInput,
  type DiagnosticLogTimeRange,
} from "../../lib/diagnostics/logView";
import {
  buildDiagnosticStatusCards,
  type AgentLogLine,
  type DiagnosticsStatusDto,
  type LogLevelFilter,
  type LogScope,
  type LogSourceFilter,
  type Translate,
} from "../../lib/settings/diagnosticsModel.ts";

export type { DiagnosticLogTimeRange } from "../../lib/diagnostics/logView";
export {
  buildDiagnosticStatusCards,
  diagnosticLogLevel,
  type DiagnosticsStatusDto,
  type LogSourceFilter,
} from "../../lib/settings/diagnosticsModel.ts";

export function useDiagnosticsSettings({
  active,
  activeSessionId,
  t,
}: {
  active: boolean;
  activeSessionId?: string | null;
  t: Translate;
}) {
  const hasSession = Boolean(activeSessionId);
  const [scope, setScope] = useState<LogScope>(hasSession ? "current" : "all");
  const [level, setLevel] = useState<LogLevelFilter>("all");
  const [lines, setLines] = useState(50);
  const [source, setSource] = useState<LogSourceFilter>("both");
  const [timeRange, setTimeRange] = useState<DiagnosticLogTimeRange>("1h");
  const [customSince, setCustomSince] = useState(() =>
    toLocalDateTimeInput(Date.now() - 60 * 60 * 1_000),
  );
  const [customUntil, setCustomUntil] = useState(() =>
    toLocalDateTimeInput(Date.now()),
  );
  const [liveLogs, setLiveLogs] = useState(true);
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
  const [logsUpdatedAt, setLogsUpdatedAt] = useState<string | null>(null);
  const logRequestGenerationRef = useRef(0);
  const logRequestInFlightRef = useRef(false);
  const logRowsSignatureRef = useRef("");
  const logListRef = useRef<HTMLUListElement | null>(null);
  const refreshRef = useRef<(silent?: boolean) => Promise<void>>(
    async () => {},
  );

  const normalizedLogSearch = logSearch.trim().toLowerCase();
  const visibleLogRows = normalizedLogSearch
    ? rows.filter((row) =>
        `${row.source} ${row.raw}`.toLowerCase().includes(normalizedLogSearch),
      )
    : rows;

  async function refreshLogs(silent = false) {
    if (silent && logRequestInFlightRef.current) return;
    const generation = ++logRequestGenerationRef.current;
    const manual = manualSession.trim();
    const effectiveSession =
      manual || (scope === "current" ? (activeSessionId ?? null) : null);
    const { sinceMs, untilMs } = diagnosticLogTimeBounds(
      timeRange,
      customSince,
      customUntil,
    );
    if (sinceMs != null && untilMs != null && sinceMs > untilMs) {
      logRequestInFlightRef.current = false;
      if (!silent) setBusy(false);
      setErrorMsg(t("prefs.diag.time.invalid"));
      return;
    }
    logRequestInFlightRef.current = true;
    const shouldFollow = liveLogs && (logListRef.current?.scrollTop ?? 0) < 24;
    if (!silent) setBusy(true);
    setErrorMsg("");
    try {
      const result = await invoke<AgentLogLine[]>("query_agent_logs", {
        args: {
          sessionId: effectiveSession || null,
          turnId: turnId.trim() || null,
          source,
          lines,
          minLevel: level === "issues" ? "WARN" : null,
          sinceMs,
          untilMs,
        },
      });
      if (generation !== logRequestGenerationRef.current) return;
      const signature = result
        .map((row) => `${row.timestamp}\u0000${row.source}\u0000${row.raw}`)
        .join("\u0001");
      if (signature !== logRowsSignatureRef.current) {
        logRowsSignatureRef.current = signature;
        setRows(result);
        if (shouldFollow) {
          window.requestAnimationFrame(() =>
            logListRef.current?.scrollTo(0, 0),
          );
        }
      }
      setQueried(true);
      setLogsUpdatedAt(new Date().toISOString());
    } catch (error) {
      if (generation !== logRequestGenerationRef.current) return;
      logRowsSignatureRef.current = "";
      setRows([]);
      setQueried(true);
      setErrorMsg(error instanceof Error ? error.message : String(error));
    } finally {
      if (generation === logRequestGenerationRef.current) {
        logRequestInFlightRef.current = false;
        if (!silent) setBusy(false);
      }
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
    if (!active) return;
    const timer = setTimeout(() => void refreshRef.current(), 250);
    return () => clearTimeout(timer);
  }, [
    active,
    scope,
    level,
    lines,
    source,
    timeRange,
    customSince,
    customUntil,
    manualSession,
    turnId,
  ]);

  useEffect(() => {
    if (!active || !liveLogs) return;
    const timer = window.setInterval(
      () => void refreshRef.current(true),
      1_500,
    );
    return () => window.clearInterval(timer);
  }, [active, liveLogs]);

  useEffect(
    () => () => {
      logRequestGenerationRef.current += 1;
      logRequestInFlightRef.current = false;
    },
    [],
  );

  useEffect(() => {
    if (active) void refreshDiagnosticsStatus();
  }, [active, refreshDiagnosticsStatus]);

  useEffect(() => {
    if (!logsCopied) return;
    const timer = window.setTimeout(() => setLogsCopied(false), 1_800);
    return () => window.clearTimeout(timer);
  }, [logsCopied]);

  async function copyLogs() {
    const text = visibleLogRows
      .map((row) => `[${row.source}] ${row.raw}`)
      .join("\n");
    try {
      await navigator.clipboard.writeText(text);
      setLogsCopied(true);
    } catch (error) {
      setErrorMsg(error instanceof Error ? error.message : String(error));
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

  return {
    hasSession,
    scope,
    setScope,
    level,
    setLevel,
    lines,
    setLines,
    source,
    setSource,
    timeRange,
    setTimeRange,
    customSince,
    setCustomSince,
    customUntil,
    setCustomUntil,
    liveLogs,
    setLiveLogs,
    manualSession,
    setManualSession,
    turnId,
    setTurnId,
    showAdvanced,
    setShowAdvanced,
    logSearch,
    setLogSearch,
    logsCopied,
    rows,
    visibleLogRows,
    diagnosticCards: buildDiagnosticStatusCards(
      diagnosticsStatus,
      diagnosticsStatusBusy,
      diagnosticsStatusError,
      t,
    ),
    diagnosticsStatusBusy,
    exportingDiagnostics,
    diagnosticsExportPath,
    busy,
    errorMsg,
    queried,
    logsUpdatedAt,
    logListRef,
    refreshLogs,
    refreshDiagnosticsStatus,
    copyLogs,
    exportDiagnostics,
  };
}
