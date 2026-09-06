import { invoke } from "@tauri-apps/api/core";
import {
  CheckCircle2,
  CircleAlert,
  Download,
  RefreshCw,
  ScrollText,
  ShieldCheck,
  TerminalSquare,
  Trash2,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useConfirm } from "../../hooks/ui/DialogContext";
import { useI18n } from "../../i18n/LocaleContext";
import type { MessageKey } from "../../i18n/messages";
import "../../styles/features/security-audit.css";

type AuditSource = "permission" | "sandbox";
type AuditFilter = "all" | AuditSource;

type SecurityAuditCapability = {
  kind: string;
  targetCount: number;
};

export type SecurityAuditEvent = {
  source: AuditSource;
  id: string;
  event: string;
  sessionId: string | null;
  turnId: string | null;
  toolName: string;
  profileId: string;
  result: string | null;
  durationMs: number | null;
  createdAt: string;
  reviewer: string | null;
  scope: string | null;
  capabilities: SecurityAuditCapability[];
  backend: string | null;
  sandboxed: boolean | null;
  mode: string | null;
  networkAccess: boolean | null;
  target: string | null;
};

type SecurityAuditPage = {
  items: SecurityAuditEvent[];
  nextCursor: string | null;
};

type SecurityAuditExportResult = {
  eventCount: number;
};

type SecurityAuditRetention = {
  sources: {
    source: AuditSource;
    maxFileBytes: number;
    archiveCount: number;
    retainedFileCount: number;
    maxTotalBytes: number;
  }[];
  maxTotalBytes: number;
};

type SecurityAuditClearResult = {
  filesRemoved: number;
  bytesRemoved: number;
};

const AUDIT_PAGE_SIZE = 50;
const MAX_AUDIT_EXPORT_EVENTS = 5_000;

const FILTERS: { id: AuditFilter; key: MessageKey }[] = [
  { id: "all", key: "approvals.audit.filter.all" },
  { id: "permission", key: "approvals.audit.filter.permission" },
  { id: "sandbox", key: "approvals.audit.filter.sandbox" },
];

const EVENT_LABELS: Record<string, MessageKey> = {
  "permission.evaluated": "approvals.audit.event.permission.evaluated",
  "permission.requested": "approvals.audit.event.permission.requested",
  "permission.reviewed": "approvals.audit.event.permission.reviewed",
  "permission.granted": "approvals.audit.event.permission.granted",
  "permission.denied": "approvals.audit.event.permission.denied",
  "permission.applied": "approvals.audit.event.permission.applied",
  "sandbox.spawned": "approvals.audit.event.sandbox.spawned",
  "sandbox.denied": "approvals.audit.event.sandbox.denied",
  "sandbox.backend_unavailable":
    "approvals.audit.event.sandbox.backendUnavailable",
};

function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function eventTone(
  event: SecurityAuditEvent,
): "danger" | "success" | "neutral" {
  if (
    event.event.endsWith(".denied") ||
    event.event.endsWith("backend_unavailable")
  ) {
    return "danger";
  }
  if (
    event.event.endsWith(".spawned") ||
    event.event.endsWith(".granted") ||
    event.event.endsWith(".applied")
  ) {
    return "success";
  }
  return "neutral";
}

function formatTimestamp(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat(undefined, {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  }).format(date);
}

function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB"];
  let value = bytes / 1024;
  let unit = units[0];
  for (let index = 1; index < units.length && value >= 1024; index += 1) {
    value /= 1024;
    unit = units[index];
  }
  return `${new Intl.NumberFormat(undefined, { maximumFractionDigits: 1 }).format(value)} ${unit}`;
}

function totalCapabilityTargets(event: SecurityAuditEvent): number {
  return event.capabilities.reduce((sum, item) => sum + item.targetCount, 0);
}

export default function SecurityAuditSection({ active }: { active: boolean }) {
  const { t } = useI18n();
  const confirm = useConfirm();
  const [events, setEvents] = useState<SecurityAuditEvent[]>([]);
  const [retention, setRetention] = useState<SecurityAuditRetention | null>(
    null,
  );
  const [filter, setFilter] = useState<AuditFilter>("all");
  const [loading, setLoading] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [loadMoreError, setLoadMoreError] = useState<string | null>(null);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [exporting, setExporting] = useState(false);
  const [exportResult, setExportResult] =
    useState<SecurityAuditExportResult | null>(null);
  const [exportError, setExportError] = useState<string | null>(null);
  const [clearing, setClearing] = useState(false);
  const [clearResult, setClearResult] =
    useState<SecurityAuditClearResult | null>(null);
  const [clearError, setClearError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!active || !isTauri()) return;
    setLoading(true);
    setError(null);
    setLoadMoreError(null);
    setExportResult(null);
    setExportError(null);
    setClearResult(null);
    setClearError(null);
    try {
      const [page, nextRetention] = await Promise.all([
        invoke<SecurityAuditPage>("list_security_audit_page", {
          limit: AUDIT_PAGE_SIZE,
          cursor: null,
        }),
        invoke<SecurityAuditRetention>("get_security_audit_retention"),
      ]);
      setEvents(page.items);
      setNextCursor(page.nextCursor);
      setRetention(nextRetention);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setLoading(false);
    }
  }, [active]);

  const loadMore = useCallback(async () => {
    if (
      !active ||
      !isTauri() ||
      !nextCursor ||
      loading ||
      loadingMore ||
      clearing
    )
      return;
    setLoadingMore(true);
    setLoadMoreError(null);
    try {
      const page = await invoke<SecurityAuditPage>("list_security_audit_page", {
        limit: AUDIT_PAGE_SIZE,
        cursor: nextCursor,
      });
      setEvents((current) => {
        const seen = new Set(current.map((event) => event.id));
        return [
          ...current,
          ...page.items.filter((event) => !seen.has(event.id)),
        ];
      });
      setNextCursor(page.nextCursor);
    } catch (reason) {
      setLoadMoreError(
        reason instanceof Error ? reason.message : String(reason),
      );
    } finally {
      setLoadingMore(false);
    }
  }, [active, clearing, loading, loadingMore, nextCursor]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const visible = useMemo(
    () => events.filter((event) => filter === "all" || event.source === filter),
    [events, filter],
  );

  useEffect(() => {
    setExportResult(null);
    setExportError(null);
  }, [filter]);

  const exportVisible = useCallback(async () => {
    if (!isTauri() || visible.length === 0 || exporting || clearing) return;
    setExporting(true);
    setExportResult(null);
    setExportError(null);
    try {
      const result = await invoke<SecurityAuditExportResult | null>(
        "export_security_audits",
        {
          source: filter,
          limit: Math.min(visible.length, MAX_AUDIT_EXPORT_EVENTS),
        },
      );
      if (result) setExportResult(result);
    } catch (reason) {
      setExportError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setExporting(false);
    }
  }, [clearing, exporting, filter, visible.length]);

  const retentionSources = useMemo(() => {
    if (!retention) return null;
    return retention.sources
      .map((source) =>
        t("approvals.audit.retentionSource", {
          source: t(
            source.source === "permission"
              ? "approvals.audit.filter.permission"
              : "approvals.audit.filter.sandbox",
          ),
          files: String(source.retainedFileCount),
          size: formatFileSize(source.maxFileBytes),
        }),
      )
      .join(" · ");
  }, [retention, t]);

  const clearAll = useCallback(async () => {
    if (
      !isTauri() ||
      clearing ||
      loading ||
      loadingMore ||
      exporting ||
      events.length === 0
    )
      return;
    const approved = await confirm({
      title: t("approvals.audit.clearConfirmTitle"),
      message: t("approvals.audit.clearConfirmMessage"),
      confirmLabel: t("approvals.audit.clearConfirmAction"),
      variant: "danger",
    });
    if (!approved) return;
    setClearing(true);
    setClearResult(null);
    setClearError(null);
    setExportResult(null);
    setExportError(null);
    try {
      const result = await invoke<SecurityAuditClearResult>(
        "clear_security_audits",
      );
      setEvents([]);
      setNextCursor(null);
      setClearResult(result);
    } catch (reason) {
      setClearError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setClearing(false);
    }
  }, [clearing, confirm, events.length, exporting, loading, loadingMore, t]);

  return (
    <section className="tools-detail-section security-audit-section">
      <header className="security-audit-head">
        <div>
          <h4 className="tools-detail-label">
            <ScrollText size={15} strokeWidth={2.25} aria-hidden />
            {t("approvals.audit.title")}
          </h4>
          <p className="tools-detail-body">{t("approvals.audit.sub")}</p>
        </div>
        <div className="security-audit-actions">
          <button
            type="button"
            className="mcp-btn-ghost"
            onClick={() => void exportVisible()}
            disabled={
              loading ||
              loadingMore ||
              exporting ||
              clearing ||
              visible.length === 0
            }
            aria-label={t("approvals.audit.export")}
            title={t("approvals.audit.exportHint")}
          >
            <Download size={14} aria-hidden />
            {exporting
              ? t("approvals.audit.exporting")
              : t("approvals.audit.export")}
          </button>
          <button
            type="button"
            className="mcp-btn-ghost security-audit-refresh"
            onClick={() => void refresh()}
            disabled={loading || loadingMore || exporting || clearing}
            aria-label={t("approvals.audit.refresh")}
            title={t("approvals.audit.refresh")}
          >
            <RefreshCw
              size={14}
              className={loading ? "is-spinning" : ""}
              aria-hidden
            />
            {t("approvals.audit.refresh")}
          </button>
        </div>
      </header>

      {retention && retentionSources && (
        <div className="security-audit-retention">
          <p>
            <span>{t("approvals.audit.retentionLabel")}</span>
            {t("approvals.audit.retention", {
              sources: retentionSources,
              total: formatFileSize(retention.maxTotalBytes),
            })}
          </p>
          <button
            type="button"
            className="mcp-btn-ghost security-audit-clear"
            onClick={() => void clearAll()}
            disabled={
              loading ||
              loadingMore ||
              exporting ||
              clearing ||
              events.length === 0
            }
          >
            <Trash2 size={14} aria-hidden />
            {clearing
              ? t("approvals.audit.clearing")
              : t("approvals.audit.clear")}
          </button>
        </div>
      )}

      <div className="security-audit-toolbar">
        <div
          className="security-audit-filters"
          role="tablist"
          aria-label={t("approvals.audit.title")}
        >
          {FILTERS.map(({ id, key }) => {
            const count =
              id === "all"
                ? events.length
                : events.filter((event) => event.source === id).length;
            return (
              <button
                key={id}
                type="button"
                role="tab"
                aria-selected={filter === id}
                className={filter === id ? "is-active" : ""}
                onClick={() => setFilter(id)}
              >
                {t(key)}
                <span>{count}</span>
              </button>
            );
          })}
        </div>
        <span className="security-audit-count">
          {t("approvals.audit.count", { n: String(visible.length) })}
        </span>
      </div>

      {(exportResult || exportError) && (
        <p
          className={`security-audit-export-status ${exportError ? "is-error" : ""}`}
          role={exportError ? "alert" : "status"}
          aria-live="polite"
        >
          {exportError ? (
            <>
              <CircleAlert size={14} aria-hidden />
              {t("approvals.audit.exportError")}: {exportError}
            </>
          ) : (
            <>
              <CheckCircle2 size={14} aria-hidden />
              {t("approvals.audit.exported", {
                n: String(exportResult?.eventCount ?? 0),
              })}
            </>
          )}
        </p>
      )}

      {(clearResult || clearError) && (
        <p
          className={`security-audit-export-status ${clearError ? "is-error" : ""}`}
          role={clearError ? "alert" : "status"}
          aria-live="polite"
        >
          {clearError ? (
            <>
              <CircleAlert size={14} aria-hidden />
              {t("approvals.audit.clearError")}: {clearError}
            </>
          ) : (
            <>
              <CheckCircle2 size={14} aria-hidden />
              {t("approvals.audit.cleared", {
                files: String(clearResult?.filesRemoved ?? 0),
                size: formatFileSize(clearResult?.bytesRemoved ?? 0),
              })}
            </>
          )}
        </p>
      )}

      {error ? (
        <p className="security-audit-state is-error" role="alert">
          <CircleAlert size={15} aria-hidden />
          {t("approvals.audit.error")}: {error}
        </p>
      ) : loading && events.length === 0 ? (
        <p className="security-audit-state">{t("approvals.audit.loading")}</p>
      ) : visible.length === 0 ? (
        <p className="security-audit-state">{t("approvals.audit.empty")}</p>
      ) : (
        <ol className="security-audit-list">
          {visible.map((event) => {
            const tone = eventTone(event);
            const EventIcon =
              tone === "danger"
                ? CircleAlert
                : tone === "success"
                  ? CheckCircle2
                  : event.source === "sandbox"
                    ? TerminalSquare
                    : ShieldCheck;
            const capabilityTargets = totalCapabilityTargets(event);
            return (
              <li
                key={event.id}
                className="security-audit-item"
                data-tone={tone}
              >
                <span className="security-audit-icon" aria-hidden>
                  <EventIcon size={15} strokeWidth={2.2} />
                </span>
                <div className="security-audit-content">
                  <div className="security-audit-title-row">
                    <strong>
                      {EVENT_LABELS[event.event]
                        ? t(EVENT_LABELS[event.event])
                        : event.event}
                    </strong>
                    <time dateTime={event.createdAt} title={event.createdAt}>
                      {formatTimestamp(event.createdAt)}
                    </time>
                  </div>
                  <div className="security-audit-meta">
                    <code>{event.toolName}</code>
                    <span>{event.profileId}</span>
                    {event.backend && <span>{event.backend}</span>}
                    {event.target && <span>{event.target}</span>}
                    {capabilityTargets > 0 && (
                      <span>
                        {t("approvals.audit.targets", {
                          n: String(capabilityTargets),
                        })}
                      </span>
                    )}
                    {event.durationMs != null && (
                      <span>
                        {t("approvals.audit.duration", {
                          n: String(event.durationMs),
                        })}
                      </span>
                    )}
                  </div>
                </div>
              </li>
            );
          })}
        </ol>
      )}

      {!error && loadMoreError && events.length > 0 && (
        <p className="security-audit-load-error" role="alert">
          <CircleAlert size={14} aria-hidden />
          {t("approvals.audit.error")}: {loadMoreError}
        </p>
      )}

      {!error && events.length > 0 && nextCursor && (
        <div className="security-audit-footer">
          <button
            type="button"
            className="mcp-btn-ghost"
            onClick={() => void loadMore()}
            disabled={loading || loadingMore || clearing}
          >
            {loadingMore
              ? t("approvals.audit.loadingMore")
              : t("approvals.audit.loadMore")}
          </button>
        </div>
      )}
    </section>
  );
}
