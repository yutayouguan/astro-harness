import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import {
  CheckCircle2,
  CircleHelp,
  Copy,
  Download,
  Info,
  Pause,
  Play,
  RefreshCw,
  Search,
  SlidersHorizontal,
  TriangleAlert,
  X,
  XCircle,
} from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";
import {
  useDiagnosticsSettings,
  diagnosticLogLevel,
  type DiagnosticLogTimeRange,
  type LogSourceFilter,
} from "../../hooks/settings/useDiagnosticsSettings";
import {
  formatDiagnosticTimestamp,
  presentDiagnosticMessage,
  sameDiagnosticLog,
} from "../../lib/diagnostics/logView";
import type {
  AgentLogLine,
  DiagnosticStatusCardModel,
  LogScope,
  LogLevelFilter,
} from "../../lib/settings/diagnosticsModel";
import { SelectMenu } from "../ui/SelectMenu";
import { SegmentedTabs } from "../ui/SegmentedTabs";
import StorageDiagnostics from "./StorageDiagnostics";
import "../../styles/features/settings/diagnostics-workspace.css";

const LINE_PRESETS = [50, 100, 200, 500] as const;
const COPY = {
  zh: {
    tabs: "诊断内容",
    storage: "存储与配置",
    refresh: "刷新状态",
    details: "日志详情",
    close: "关闭详情",
    raw: "原始日志",
    copy: "复制原文",
    message: "内容",
    status: "组件状态",
    follow: "实时跟随",
    pause: "暂停跟随",
    waiting: "正在读取日志…",
  },
  en: {
    tabs: "Diagnostics views",
    storage: "Storage & configuration",
    refresh: "Refresh status",
    details: "Log details",
    close: "Close details",
    raw: "Raw log",
    copy: "Copy raw log",
    message: "Message",
    status: "Component status",
    follow: "Follow live",
    pause: "Pause live",
    waiting: "Loading logs…",
  },
} as const;

function Filter({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="diagnostics-filter">
      <span>{label}</span>
      {children}
    </div>
  );
}

function StatusItem({
  card,
  selected,
  onSelect,
  disabled,
}: {
  card: DiagnosticStatusCardModel;
  selected: boolean;
  onSelect: () => void;
  disabled: boolean;
}) {
  const Icon =
    card.state === "healthy"
      ? CheckCircle2
      : card.state === "error"
        ? XCircle
        : card.state === "warning"
          ? TriangleAlert
          : card.state === "configured"
            ? Info
            : CircleHelp;
  return (
    <button
      type="button"
      className="diagnostics-status-item"
      data-status={card.state}
      aria-expanded={selected}
      onClick={onSelect}
      disabled={disabled}
    >
      <Icon size={15} aria-hidden />
      <span>{card.label}</span>
      <strong>{card.value}</strong>
    </button>
  );
}

export default function DiagnosticsPanel({
  active,
  activeSessionId,
  references,
}: {
  active: boolean;
  activeSessionId?: string | null;
  references: string[];
}) {
  const { t, locale } = useI18n();
  const copy = COPY[locale];
  const [tab, setTab] = useState("logs");
  const [statusId, setStatusId] = useState<string | null>(null);
  const [selectedLog, setSelectedLog] = useState<AgentLogLine | null>(null);
  const [storageBusy, setStorageBusy] = useState(false);
  const [copiedRaw, setCopiedRaw] = useState(false);
  const [copyError, setCopyError] = useState("");
  const selectionGeneration = useRef(0);
  const selectedTrigger = useRef<HTMLButtonElement | null>(null);
  const closeButton = useRef<HTMLButtonElement | null>(null);
  useEffect(() => {
    if (selectedLog) closeButton.current?.focus();
  }, [selectedLog]);
  const id = useId();
  const d = useDiagnosticsSettings({
    active,
    logsActive: active && tab === "logs",
    activeSessionId,
    t,
  });
  const selectedStatus = d.diagnosticCards.find((card) => card.id === statusId);
  const advancedCount =
    Number(d.lines !== 50) +
    Number(Boolean(d.manualSession.trim())) +
    Number(Boolean(d.turnId.trim())) +
    Number(d.timeRange === "custom");
  const closeDetails = () => {
    selectionGeneration.current++;
    setSelectedLog(null);
    setCopiedRaw(false);
    setCopyError("");
    if (selectedTrigger.current?.isConnected) selectedTrigger.current.focus();
    else d.logListRef.current?.focus();
  };
  const copyRaw = async () => {
    if (!selectedLog) return;
    const generation = selectionGeneration.current;
    try {
      await navigator.clipboard.writeText(selectedLog.raw);
      if (generation === selectionGeneration.current) setCopiedRaw(true);
    } catch {
      if (generation === selectionGeneration.current)
        setCopyError(t("prefs.diag.copyFailed"));
    }
  };

  return (
    <div className="diagnostics-workspace">
      <header className="diagnostics-header">
        <h2>{t("prefs.category.diagnostics")}</h2>
        <div className="diagnostics-header-actions">
          <button
            type="button"
            className="prefs-diag-btn"
            disabled={d.diagnosticsStatusBusy || storageBusy}
            onClick={() => void d.refreshDiagnosticsStatus()}
          >
            <RefreshCw
              size={14}
              className={d.diagnosticsStatusBusy ? "spin" : undefined}
              aria-hidden
            />
            {copy.refresh}
          </button>
          <button
            type="button"
            className="prefs-diag-btn"
            disabled={d.exportingDiagnostics}
            onClick={() => void d.exportDiagnostics()}
            title={t("prefs.diag.export.sub")}
          >
            <Download size={14} aria-hidden />
            {t(
              d.exportingDiagnostics
                ? "prefs.diag.export.exporting"
                : "prefs.diag.export.action",
            )}
          </button>
        </div>
      </header>
      <div className="diagnostics-status-strip" aria-label={copy.status}>
        {d.diagnosticCards.map((card) => (
          <StatusItem
            key={card.id}
            card={card}
            disabled={d.diagnosticsStatusBusy}
            selected={statusId === card.id}
            onSelect={() => setStatusId(statusId === card.id ? null : card.id)}
          />
        ))}
      </div>
      {selectedStatus && (
        <div className="diagnostics-status-detail" role="status">
          <Info size={14} aria-hidden />
          <span>
            <strong>
              {selectedStatus.label} · {selectedStatus.value}
            </strong>
            {selectedStatus.detail}
          </span>
          <button
            type="button"
            className="diagnostics-icon-button"
            onClick={() => setStatusId(null)}
            aria-label={copy.close}
          >
            <X size={16} />
          </button>
        </div>
      )}
      {d.errorMsg && (
        <p className="prefs-diag-error" role="alert">
          {d.errorMsg}
        </p>
      )}
      {d.diagnosticsExportPath && (
        <p className="diagnostics-export-result" role="status">
          {t("prefs.diag.export.done", { path: d.diagnosticsExportPath })}
        </p>
      )}
      <SegmentedTabs
        className="diagnostics-tabs"
        size="sm"
        aria-label={copy.tabs}
        value={tab}
        onValueChange={setTab}
        items={[
          {
            value: "logs",
            label: t("prefs.diag.title"),
            panelId: `${id}-logs`,
            disabled: storageBusy,
          },
          {
            value: "storage",
            label: copy.storage,
            panelId: `${id}-storage`,
            disabled: storageBusy,
          },
        ]}
      />
      <section
        id={`${id}-logs`}
        className="diagnostics-log-panel"
        role="tabpanel"
        aria-label={t("prefs.diag.title")}
        hidden={tab !== "logs"}
      >
        <div className="diagnostics-filters">
          <Filter label={t("prefs.diag.scope")}>
            <SelectMenu
              value={d.scope}
              aria-label={t("prefs.diag.scope")}
              onChange={(value) => d.setScope(value as LogScope)}
              options={[
                ...(d.hasSession
                  ? [{ value: "current", label: t("prefs.diag.scope.current") }]
                  : []),
                { value: "all", label: t("prefs.diag.scope.all") },
              ]}
            />
          </Filter>
          <Filter label={t("prefs.diag.time")}>
            <SelectMenu
              value={d.timeRange}
              aria-label={t("prefs.diag.time")}
              onChange={(value) => {
                d.setTimeRange(value as DiagnosticLogTimeRange);
                if (value === "custom") {
                  d.setLiveLogs(false);
                  d.setShowAdvanced(true);
                }
              }}
              options={(
                ["15m", "1h", "24h", "7d", "all", "custom"] as const
              ).map((value) => ({
                value,
                label: t(`prefs.diag.time.${value}`),
              }))}
            />
          </Filter>
          <Filter label={t("prefs.diag.source")}>
            <SelectMenu
              value={d.source}
              aria-label={t("prefs.diag.source")}
              onChange={(value) => d.setSource(value as LogSourceFilter)}
              options={(["both", "agent", "errors"] as const).map((value) => ({
                value,
                label: t(`prefs.diag.source.${value}`),
              }))}
            />
          </Filter>
          <Filter label={t("prefs.diag.level")}>
            <SelectMenu
              value={d.level}
              aria-label={t("prefs.diag.level")}
              onChange={(value) => d.setLevel(value as LogLevelFilter)}
              options={(["all", "issues"] as const).map((value) => ({
                value,
                label: t(`prefs.diag.level.${value}`),
              }))}
            />
          </Filter>
          <label className="diagnostics-search" data-input-surface>
            <Search size={15} aria-hidden />
            <input
              type="search"
              aria-label={t("prefs.diag.search")}
              placeholder={t("prefs.diag.search.ph")}
              value={d.logSearch}
              onChange={(event) => d.setLogSearch(event.target.value)}
            />
          </label>
          <button
            type="button"
            className="diagnostics-icon-button"
            aria-label={t("prefs.diag.advanced.show")}
            title={t("prefs.diag.advanced.show")}
            aria-expanded={d.showAdvanced}
            data-active={advancedCount > 0 || undefined}
            aria-description={
              advancedCount
                ? locale === "zh"
                  ? `已设置 ${advancedCount} 项筛选`
                  : `${advancedCount} active filters`
                : undefined
            }
            aria-controls={`${id}-advanced`}
            onClick={() => d.setShowAdvanced(!d.showAdvanced)}
          >
            <SlidersHorizontal size={17} />
            {advancedCount > 0 && (
              <span className="diagnostics-filter-count" aria-hidden>
                {advancedCount}
              </span>
            )}
          </button>
        </div>
        {d.showAdvanced && (
          <div id={`${id}-advanced`} className="diagnostics-advanced">
            <Filter label={t("prefs.diag.lines")}>
              <SelectMenu
                value={String(d.lines)}
                aria-label={t("prefs.diag.lines")}
                onChange={(value) => d.setLines(Number(value))}
                options={LINE_PRESETS.map((count) => ({
                  value: String(count),
                  label: String(count),
                }))}
              />
            </Filter>
            <label>
              {t("prefs.diag.session")}
              <input
                className="prefs-diag-input"
                value={d.manualSession}
                placeholder={t("prefs.diag.session.ph")}
                onChange={(event) => d.setManualSession(event.target.value)}
                spellCheck={false}
                autoComplete="off"
              />
            </label>
            <label>
              {t("prefs.diag.turn")}
              <input
                className="prefs-diag-input"
                value={d.turnId}
                placeholder={t("prefs.diag.turn.ph")}
                onChange={(event) => d.setTurnId(event.target.value)}
                spellCheck={false}
                autoComplete="off"
              />
            </label>
            {d.timeRange === "custom" && (
              <>
                <label>
                  {t("prefs.diag.time.start")}
                  <input
                    className="prefs-diag-input"
                    type="datetime-local"
                    value={d.customSince}
                    onChange={(event) => d.setCustomSince(event.target.value)}
                  />
                </label>
                <label>
                  {t("prefs.diag.time.end")}
                  <input
                    className="prefs-diag-input"
                    type="datetime-local"
                    value={d.customUntil}
                    onChange={(event) => d.setCustomUntil(event.target.value)}
                  />
                </label>
              </>
            )}
          </div>
        )}
        <div className="diagnostics-log-toolbar">
          <span role="status" aria-live={d.liveLogs ? "off" : "polite"}>
            {t("prefs.diag.results", {
              shown: String(d.visibleLogRows.length),
              total: String(d.rows.length),
            })}
          </span>
          <div>
            <button
              type="button"
              className="prefs-diag-btn"
              disabled={d.busy}
              onClick={() => void d.refreshLogs()}
              title={t("prefs.diag.refresh")}
            >
              <RefreshCw size={13} aria-hidden />
              {t("prefs.diag.refresh")}
            </button>
            <button
              type="button"
              className="prefs-diag-btn"
              aria-pressed={d.liveLogs}
              onClick={() => {
                const next = !d.liveLogs;
                d.setLiveLogs(next);
                if (next && d.timeRange === "custom") d.setTimeRange("1h");
              }}
            >
              {d.liveLogs ? (
                <Pause size={13} aria-hidden />
              ) : (
                <Play size={13} aria-hidden />
              )}
              {d.liveLogs ? copy.pause : copy.follow}
            </button>
            <button
              type="button"
              className="prefs-diag-btn"
              disabled={!d.visibleLogRows.length}
              onClick={() => void d.copyLogs()}
            >
              <Copy size={13} aria-hidden />
              {t(d.logsCopied ? "prefs.diag.copied" : "prefs.diag.copyVisible")}
            </button>
          </div>
        </div>
        <div
          className={`diagnostics-log-body${selectedLog ? " has-detail" : ""}`}
        >
          <div className="diagnostics-log-table" aria-busy={d.busy}>
            <div className="diagnostics-log-columns" aria-hidden>
              <span>{t("prefs.diag.time")}</span>
              <span>{t("prefs.diag.level")}</span>
              <span>{t("prefs.diag.source")}</span>
              <span>{copy.message}</span>
            </div>
            <ul
              ref={d.logListRef}
              tabIndex={-1}
              className="diagnostics-log-list"
              aria-label={t("prefs.diag.title")}
            >
              {d.visibleLogRows.map((row, index) => (
                <li key={`${row.timestamp}:${row.source}:${index}`}>
                  <button
                    type="button"
                    className="diagnostics-log-entry"
                    data-severity={diagnosticLogLevel(row.level)}
                    aria-expanded={sameDiagnosticLog(selectedLog, row)}
                    aria-controls={`${id}-detail`}
                    onClick={(event) => {
                      selectionGeneration.current++;
                      selectedTrigger.current = event.currentTarget;
                      setSelectedLog(row);
                      setCopiedRaw(false);
                      setCopyError("");
                    }}
                  >
                    <time dateTime={row.timestamp}>
                      {formatDiagnosticTimestamp(row.timestamp, locale)}
                    </time>
                    <span className="diagnostics-severity">{row.level}</span>
                    <span className="diagnostics-source">{row.source}</span>
                    <span className="diagnostics-message">
                      {presentDiagnosticMessage(row.message)}
                    </span>
                  </button>
                </li>
              ))}
              {!d.visibleLogRows.length && (
                <li className="diagnostics-empty">
                  {d.errorMsg
                    ? d.errorMsg
                    : d.busy || !d.queried
                      ? copy.waiting
                      : t(
                          d.rows.length
                            ? "prefs.diag.emptySearch"
                            : "prefs.diag.empty",
                        )}
                </li>
              )}
            </ul>
          </div>
          {selectedLog && (
            <aside
              id={`${id}-detail`}
              className="diagnostics-log-detail"
              aria-label={copy.details}
              onKeyDown={(event) => {
                if (event.key === "Escape") {
                  event.stopPropagation();
                  closeDetails();
                }
              }}
            >
              <header>
                <h3>{copy.details}</h3>
                <button
                  type="button"
                  className="diagnostics-icon-button"
                  aria-label={copy.close}
                  ref={closeButton}
                  onClick={closeDetails}
                >
                  <X size={16} />
                </button>
              </header>
              <time dateTime={selectedLog.timestamp}>
                {formatDiagnosticTimestamp(selectedLog.timestamp, locale)} ·{" "}
                {selectedLog.level} · {selectedLog.source}
              </time>
              <pre tabIndex={0}>
                {presentDiagnosticMessage(selectedLog.message)}
              </pre>
              <details>
                <summary>{copy.raw}</summary>
                <pre tabIndex={0}>{selectedLog.raw}</pre>
              </details>
              <button
                type="button"
                className="prefs-diag-btn"
                onClick={() => void copyRaw()}
              >
                {copiedRaw ? t("prefs.diag.copied") : copy.copy}
              </button>
              {copyError && <p role="alert">{copyError}</p>}
            </aside>
          )}
        </div>
        <footer className="diagnostics-log-footer">
          <span>{t("prefs.diag.newestFirst")}</span>
          <span>
            {d.logsUpdatedAt &&
              t("prefs.diag.updated", {
                time: formatDiagnosticTimestamp(d.logsUpdatedAt, locale),
              })}
          </span>
        </footer>
      </section>
      <section
        id={`${id}-storage`}
        className="diagnostics-storage-panel"
        role="tabpanel"
        aria-label={copy.storage}
        hidden={tab !== "storage"}
      >
        <StorageDiagnostics
          active={active && tab === "storage"}
          references={references}
          onBusyChange={setStorageBusy}
        />
      </section>
    </div>
  );
}
