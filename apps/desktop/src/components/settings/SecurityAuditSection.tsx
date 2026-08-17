import { invoke } from "@tauri-apps/api/core";
import {
  CheckCircle2,
  CircleAlert,
  RefreshCw,
  ScrollText,
  ShieldCheck,
  TerminalSquare,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
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

function eventTone(event: SecurityAuditEvent): "danger" | "success" | "neutral" {
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

function totalCapabilityTargets(event: SecurityAuditEvent): number {
  return event.capabilities.reduce((sum, item) => sum + item.targetCount, 0);
}

export default function SecurityAuditSection({ active }: { active: boolean }) {
  const { t } = useI18n();
  const [events, setEvents] = useState<SecurityAuditEvent[]>([]);
  const [filter, setFilter] = useState<AuditFilter>("all");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!active || !isTauri()) return;
    setLoading(true);
    setError(null);
    try {
      setEvents(await invoke<SecurityAuditEvent[]>("list_security_audits", { limit: 100 }));
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setLoading(false);
    }
  }, [active]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const visible = useMemo(
    () => events.filter((event) => filter === "all" || event.source === filter),
    [events, filter],
  );

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
        <button
          type="button"
          className="mcp-btn-ghost security-audit-refresh"
          onClick={() => void refresh()}
          disabled={loading}
          aria-label={t("approvals.audit.refresh")}
          title={t("approvals.audit.refresh")}
        >
          <RefreshCw size={14} className={loading ? "is-spinning" : ""} aria-hidden />
          {t("approvals.audit.refresh")}
        </button>
      </header>

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
              <li key={event.id} className="security-audit-item" data-tone={tone}>
                <span className="security-audit-icon" aria-hidden>
                  <EventIcon size={15} strokeWidth={2.2} />
                </span>
                <div className="security-audit-content">
                  <div className="security-audit-title-row">
                    <strong>
                      {EVENT_LABELS[event.event] ? t(EVENT_LABELS[event.event]) : event.event}
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
                        {t("approvals.audit.targets", { n: String(capabilityTargets) })}
                      </span>
                    )}
                    {event.durationMs != null && (
                      <span>{t("approvals.audit.duration", { n: String(event.durationMs) })}</span>
                    )}
                  </div>
                </div>
              </li>
            );
          })}
        </ol>
      )}
    </section>
  );
}
