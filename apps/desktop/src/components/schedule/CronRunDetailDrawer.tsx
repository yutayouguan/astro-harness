import { useCallback, useId, useState } from "react";
import {
  Archive,
  ArchiveRestore,
  CalendarClock,
  CheckCircle2,
  ChevronRight,
  CircleAlert,
  Clock3,
  Eye,
  EyeOff,
  History,
  ListTree,
  LoaderCircle,
  MessageSquareText,
  Pause,
  Pencil,
  Play,
  TerminalSquare,
  X,
} from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";
import { formatScheduleLabel } from "../../lib/cron/cronSchedule";
import type { ConversationEntry } from "../../types";
import { CopyMorphIcon } from "../icons/MorphIcon";
import { ChatMarkdown } from "../chat/ChatMarkdown";
import MsgActivity from "../chat/MsgActivity";
import { Drawer } from "../ui";

export type CronRunDto = {
  id: string;
  job_id: string;
  title: string;
  agent_id: string;
  schedule: string;
  task: string;
  fired_at: string;
  finished_at: string | null;
  status: string;
  summary: string;
  output: string;
  error: string | null;
  session_id: string | null;
  trigger: string;
};

export type CronJobDto = {
  id: string;
  schedule: string;
  task: string;
  title: string;
  agent_id: string;
  provider_id: string | null;
  model: string | null;
  enabled: boolean;
  created_at: string;
  last_run_at: string | null;
  next_run_at: string | null;
  show_in_chat: boolean;
  /** 归档时间；非空表示任务已归档（不参与调度，可恢复） */
  archived_at: string | null;
};

export function cronRunStatusKind(
  status: string,
): "success" | "failure" | "running" | "other" {
  const normalized = status.toLowerCase();
  if (["success", "ok", "completed"].includes(normalized)) return "success";
  if (["failure", "failed", "error"].includes(normalized)) return "failure";
  if (["running", "in_progress", "pending"].includes(normalized))
    return "running";
  return "other";
}

export function formatCronRunTime(iso: string, locale: "zh" | "en"): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  return date.toLocaleString(locale === "zh" ? "zh-CN" : "en-US", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function CronRunStatusIcon({ status }: { status: string }) {
  const kind = cronRunStatusKind(status);
  if (kind === "success") {
    return <CheckCircle2 size={12} strokeWidth={2.4} aria-hidden />;
  }
  if (kind === "failure") {
    return <CircleAlert size={12} strokeWidth={2.4} aria-hidden />;
  }
  if (kind === "running") {
    return (
      <LoaderCircle
        size={12}
        strokeWidth={2.4}
        className="is-spin"
        aria-hidden
      />
    );
  }
  return <Clock3 size={12} strokeWidth={2.4} aria-hidden />;
}

function useCronRunStatusLabel() {
  const { t } = useI18n();
  return useCallback(
    (status: string) => {
      const kind = cronRunStatusKind(status);
      if (kind === "success") return t("cron.run.statusSuccess");
      if (kind === "failure") return t("cron.run.statusFailure");
      if (kind === "running") return t("cron.run.statusRunning");
      if (status === "manual") return t("cron.run.statusManual");
      if (status === "due") return t("cron.run.statusDue");
      return status;
    },
    [t],
  );
}

function CopyLogButton({ text }: { text: string }) {
  const { t } = useI18n();
  const [copied, setCopied] = useState(false);

  const onCopy = useCallback(async () => {
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1600);
    } catch {
      // Clipboard access can be unavailable in preview contexts.
    }
  }, [text]);

  return (
    <button
      type="button"
      className={`cron-run-drawer-copy ${copied ? "is-copied" : ""}`}
      onClick={() => void onCopy()}
      aria-label={copied ? t("chat.copied") : t("chat.copy")}
      title={copied ? t("chat.copied") : t("chat.copy")}
    >
      <CopyMorphIcon copied={copied} size={13} aria-hidden />
    </button>
  );
}

export function CronRunFloatingCard({
  run,
  onOpen,
}: {
  run: CronRunDto;
  onOpen: () => void;
}) {
  const { t, locale } = useI18n();
  const statusLabel = useCronRunStatusLabel();
  const description = run.summary || run.output || run.error || run.task;

  return (
    <button
      type="button"
      className="chat-cron-run-card"
      onClick={onOpen}
      aria-label={`${t("cron.view.detail")}: ${run.title}`}
    >
      <span className="chat-cron-run-card-icon" aria-hidden>
        <CalendarClock size={17} strokeWidth={2.2} />
      </span>
      <span className="chat-cron-run-card-copy">
        <span className="chat-cron-run-card-title">{run.title}</span>
        {description ? (
          <span className="chat-cron-run-card-summary">{description}</span>
        ) : null}
        <span className="chat-cron-run-card-time">
          {formatCronRunTime(run.fired_at, locale)}
        </span>
      </span>
      <span className={`cron-status-pill is-${cronRunStatusKind(run.status)}`}>
        <CronRunStatusIcon status={run.status} />
        {statusLabel(run.status)}
      </span>
      <ChevronRight size={16} strokeWidth={2.2} aria-hidden />
    </button>
  );
}

type TaskDrawerProps = {
  job: CronJobDto;
  runs: CronRunDto[];
  runsLoading?: boolean;
  busy?: boolean;
  nonModal?: boolean;
  onClose: () => void;
  onEdit: () => void;
  onToggleEnabled: () => void;
  onToggleArchived: () => void;
  onRunNow: () => void;
  onOpenRun: (run: CronRunDto) => void;
};

export function CronTaskDetailDrawer({
  job,
  runs,
  runsLoading = false,
  busy = false,
  nonModal = false,
  onClose,
  onEdit,
  onToggleEnabled,
  onToggleArchived,
  onRunNow,
  onOpenRun,
}: TaskDrawerProps) {
  const { t, locale } = useI18n();
  const titleId = useId();
  const statusLabel = useCronRunStatusLabel();
  const archived = Boolean(job.archived_at);

  return (
    <Drawer
      open
      onClose={onClose}
      size="lg"
      modal={!nonModal}
      closeOnBackdrop={!nonModal}
      trapFocus={!nonModal}
      role={nonModal ? "complementary" : "dialog"}
      backdropStyle={{
        background: "transparent",
        backdropFilter: "none",
        WebkitBackdropFilter: "none",
        pointerEvents: nonModal ? "none" : undefined,
      }}
      style={{ pointerEvents: nonModal ? "auto" : undefined }}
      className="cron-job-drawer"
      aria-labelledby={titleId}
    >
      <header className="cron-job-drawer-head">
        <div className="cron-job-drawer-heading">
          <span className="cron-job-drawer-icon" aria-hidden>
            <CalendarClock size={19} strokeWidth={2.2} />
          </span>
          <div>
            <h2 id={titleId} className="cron-job-drawer-title">
              {job.title}
            </h2>
            <span
              className={`cron-card-status ${
                archived ? "is-archived" : job.enabled ? "is-on" : "is-off"
              }`}
            >
              {archived
                ? t("cron.statusArchived")
                : job.enabled
                  ? t("cron.statusOn")
                  : t("cron.statusOff")}
            </span>
          </div>
        </div>
        <button
          type="button"
          className="cron-dialog-close"
          onClick={onClose}
          aria-label={t("cron.cancel")}
        >
          <X size={17} strokeWidth={2.4} aria-hidden />
        </button>
      </header>

      <div className="cron-job-drawer-body">
        <section className="cron-job-overview-card">
          <div className="cron-job-overview-row">
            <CalendarClock size={15} strokeWidth={2.1} aria-hidden />
            <span>{formatScheduleLabel(job.schedule, locale)}</span>
          </div>
          <div className="cron-job-overview-row is-task">
            <MessageSquareText size={15} strokeWidth={2.1} aria-hidden />
            <span>{job.task}</span>
          </div>
          <div className="cron-job-overview-row">
            {job.show_in_chat ? (
              <Eye size={15} strokeWidth={2.1} aria-hidden />
            ) : (
              <EyeOff size={15} strokeWidth={2.1} aria-hidden />
            )}
            <span>
              {t("cron.detail.showInChat")}:{" "}
              {job.show_in_chat ? t("cron.yes") : t("cron.no")}
            </span>
          </div>
        </section>

        <div className="cron-job-drawer-actions">
          <button
            type="button"
            className="cron-job-drawer-action"
            onClick={onEdit}
          >
            <Pencil size={15} strokeWidth={2.2} aria-hidden />
            <span>{t("cron.edit")}</span>
          </button>
          {archived ? (
            <button
              type="button"
              className="cron-job-drawer-action"
              disabled={busy}
              onClick={onToggleArchived}
            >
              <ArchiveRestore size={15} strokeWidth={2.2} aria-hidden />
              <span>{t("cron.restore")}</span>
            </button>
          ) : (
            <>
              <button
                type="button"
                className="cron-job-drawer-action"
                disabled={busy}
                onClick={onToggleEnabled}
              >
                {job.enabled ? (
                  <Pause size={15} strokeWidth={2.2} aria-hidden />
                ) : (
                  <CheckCircle2 size={15} strokeWidth={2.2} aria-hidden />
                )}
                <span>{job.enabled ? t("cron.pause") : t("cron.resume")}</span>
              </button>
              <button
                type="button"
                className="cron-job-drawer-action"
                disabled={busy}
                onClick={onToggleArchived}
              >
                <Archive size={15} strokeWidth={2.2} aria-hidden />
                <span>{t("cron.archive")}</span>
              </button>
              <button
                type="button"
                className="cron-job-drawer-action is-primary"
                disabled={busy}
                onClick={onRunNow}
              >
                <Play
                  size={15}
                  strokeWidth={2.2}
                  fill="currentColor"
                  aria-hidden
                />
                <span>{t("cron.runNow")}</span>
              </button>
            </>
          )}
        </div>

        <section className="cron-job-drawer-history">
          <div className="cron-job-drawer-section-head">
            <h3>
              <History size={14} strokeWidth={2.2} aria-hidden />
              {t("cron.detail.recentRuns")}
            </h3>
            <span>{runs.length}</span>
          </div>
          {runsLoading ? (
            <p className="cron-loading">{t("workspace.loading")}</p>
          ) : runs.length === 0 ? (
            <p className="cron-history-empty">{t("cron.history.empty")}</p>
          ) : (
            <ul className="cron-job-drawer-run-list">
              {runs.slice(0, 30).map((run) => (
                <li key={run.id}>
                  <button
                    type="button"
                    className="cron-job-drawer-run"
                    onClick={() => onOpenRun(run)}
                  >
                    <span className="cron-job-drawer-run-main">
                      <strong>{run.summary || run.title}</strong>
                      <small>{formatCronRunTime(run.fired_at, locale)}</small>
                    </span>
                    <span
                      className={`cron-status-pill is-${cronRunStatusKind(run.status)}`}
                    >
                      <CronRunStatusIcon status={run.status} />
                      {statusLabel(run.status)}
                    </span>
                    <ChevronRight size={15} strokeWidth={2.2} aria-hidden />
                  </button>
                </li>
              ))}
            </ul>
          )}
        </section>
      </div>
    </Drawer>
  );
}

type DrawerProps = {
  run: CronRunDto;
  messages: ConversationEntry[];
  traceLoading?: boolean;
  onClose: () => void;
  onDelete: () => void;
};

export function CronRunDetailDrawer({
  run,
  messages,
  traceLoading = false,
  onClose,
  onDelete,
}: DrawerProps) {
  const { t, locale } = useI18n();
  const titleId = useId();
  const statusLabel = useCronRunStatusLabel();
  const activities = messages.flatMap((message) => message.activities ?? []);
  const assistantTexts = messages
    .filter((message) => message.role === "assistant" && message.content.trim())
    .map((message) => message.content.trim());
  const copyText = assistantTexts[assistantTexts.length - 1] || run.output;
  const running = cronRunStatusKind(run.status) === "running";

  return (
    <Drawer
      open
      onClose={onClose}
      size="lg"
      modal={false}
      closeOnBackdrop={false}
      trapFocus={false}
      role="complementary"
      backdropStyle={{
        background: "transparent",
        backdropFilter: "none",
        WebkitBackdropFilter: "none",
        pointerEvents: "none",
      }}
      style={{ pointerEvents: "auto" }}
      className="cron-run-drawer"
      aria-labelledby={titleId}
    >
      <header className="cron-run-drawer-head">
        <div>
          <h2 id={titleId} className="cron-run-drawer-title">
            <History size={16} strokeWidth={2.3} aria-hidden />
            {t("cron.history.logTitle")}
          </h2>
          <p className="cron-run-drawer-sub">
            <CalendarClock size={12} strokeWidth={2.2} aria-hidden />
            <span>
              {run.title} · {formatCronRunTime(run.fired_at, locale)}
            </span>
          </p>
        </div>
        <div className="cron-run-drawer-head-actions">
          <button
            type="button"
            className="cron-timeline-log-btn is-danger"
            disabled={running}
            onClick={onDelete}
            title={t("cron.history.deleteRun")}
            aria-label={t("cron.history.deleteRun")}
          >
            <TrashIcon />
            <span>{t("cron.history.deleteRun")}</span>
          </button>
          <button
            type="button"
            className="cron-dialog-close"
            onClick={onClose}
            aria-label={t("cron.cancel")}
          >
            <X size={16} strokeWidth={2.5} aria-hidden />
          </button>
        </div>
      </header>
      <div className="cron-run-drawer-body">
        <div className="cron-run-drawer-meta">
          <span
            className={`cron-status-pill is-${cronRunStatusKind(run.status)}`}
          >
            <CronRunStatusIcon status={run.status} />
            {statusLabel(run.status)}
          </span>
          {run.summary ? (
            <p className="cron-run-drawer-summary">
              <MessageSquareText size={13} strokeWidth={2.2} aria-hidden />
              <span>{run.summary}</span>
            </p>
          ) : null}
        </div>

        <section className="cron-run-drawer-block">
          <div className="cron-run-drawer-block-head">
            <h3 className="cron-run-drawer-label">
              <ListTree size={12} strokeWidth={2.3} aria-hidden />
              {t("cron.history.traceTitle")}
            </h3>
            {copyText ? <CopyLogButton text={copyText} /> : null}
          </div>
          {activities.length === 0 && assistantTexts.length === 0 ? (
            <p className="cron-history-empty">
              {running
                ? t("cron.history.runningWait")
                : traceLoading
                  ? t("cron.history.loadingTrace")
                  : t("cron.history.empty")}
            </p>
          ) : (
            <div className="cron-run-trace">
              {activities.map((activity, index) => (
                <MsgActivity
                  key={activity.id || `act-${index}`}
                  activity={activity}
                  defaultOpen={activity.status === "running"}
                  showTimestamp
                />
              ))}
              {assistantTexts.map((text, index) => (
                <div key={`asst-${index}`} className="cron-run-assistant-chunk">
                  <div className="cron-run-md">
                    <ChatMarkdown content={text} compact />
                  </div>
                </div>
              ))}
            </div>
          )}
        </section>

        {run.output && assistantTexts.length === 0 ? (
          <section className="cron-run-drawer-block">
            <div className="cron-run-drawer-block-head">
              <h3 className="cron-run-drawer-label">
                <TerminalSquare size={12} strokeWidth={2.3} aria-hidden />
                Output
              </h3>
              <CopyLogButton text={run.output} />
            </div>
            <div className="cron-run-md">
              <ChatMarkdown content={run.output} compact />
            </div>
          </section>
        ) : null}
        {run.error ? (
          <section className="cron-run-drawer-block is-error">
            <div className="cron-run-drawer-block-head">
              <h3 className="cron-run-drawer-label">
                <CircleAlert size={12} strokeWidth={2.3} aria-hidden />
                Error
              </h3>
              <CopyLogButton text={run.error} />
            </div>
            <div className="cron-run-md">
              <ChatMarkdown content={run.error} plain compact />
            </div>
          </section>
        ) : null}
      </div>
    </Drawer>
  );
}

function TrashIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" aria-hidden>
      <path
        d="M4 7h16M9 7V4h6v3m-8 0 1 13h8l1-13M10 11v5m4-5v5"
        stroke="currentColor"
        strokeWidth="1.9"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}
