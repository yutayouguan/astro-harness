import { useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Bot, ChevronDown, Square } from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";
import type { SubagentThread, SubagentThreadStatus } from "../../hooks/chat/useSubagentThreads";

type Props = {
  parentSessionId?: string | null;
  threads: SubagentThread[];
  onRefresh: () => Promise<void>;
  onOpenThread: (threadId: string) => void;
  onOpenPanel: () => void;
};

const ACTIVE = new Set<SubagentThreadStatus>(["pending", "running"]);

export default function SubagentActivityBar({
  parentSessionId,
  threads,
  onRefresh,
  onOpenThread,
  onOpenPanel,
}: Props) {
  const { t } = useI18n();
  const [expanded, setExpanded] = useState(true);
  const [stopping, setStopping] = useState(false);
  const visible = useMemo(
    () => threads.filter((thread) => thread.status !== "closed"),
    [threads],
  );
  const active = useMemo(
    () => visible.filter((thread) => ACTIVE.has(thread.status)),
    [visible],
  );

  if (!parentSessionId || visible.length === 0) return null;

  const statusLabel = (status: SubagentThreadStatus) =>
    t(`subagents.status.${status}` as never);

  const stopAll = async () => {
    if (active.length === 0 || stopping) return;
    setStopping(true);
    try {
      await Promise.allSettled(
        active.map((thread) =>
          invoke("interrupt_subagent_thread", {
            args: { parentSessionId, threadId: thread.id },
          }),
        ),
      );
      await onRefresh();
    } finally {
      setStopping(false);
    }
  };

  return (
    <section className="composer-queue subagent-activity" aria-live="polite">
      <div className="subagent-activity-head">
        <button
          type="button"
          className="subagent-activity-toggle"
          aria-expanded={expanded}
          onClick={() => setExpanded((current) => !current)}
        >
          <Bot size={15} strokeWidth={2.1} aria-hidden />
          <span className="subagent-activity-copy">
            <strong>{t("subagents.activity.title", { count: String(visible.length) })}</strong>
            <small>
              {active.length > 0
                ? t("subagents.activity.running", { count: String(active.length) })
                : t("subagents.activity.done")}
            </small>
          </span>
          <ChevronDown className={expanded ? "is-open" : ""} size={14} aria-hidden />
        </button>
        <span className="subagent-activity-actions">
          {active.length > 0 ? (
            <button type="button" disabled={stopping} onClick={() => void stopAll()}>
              <Square size={11} strokeWidth={2.4} aria-hidden />
              {t("subagents.activity.stopAll")}
            </button>
          ) : null}
          <button type="button" onClick={onOpenPanel}>{t("subagents.activity.openAll")}</button>
        </span>
      </div>
      {expanded ? (
        <div className="subagent-activity-list">
          {visible.map((thread) => (
            <button
              type="button"
              key={thread.id}
              className={`is-${thread.status}`}
              onClick={() => onOpenThread(thread.id)}
            >
              <span className={`subagents-status is-${thread.status}`} aria-hidden />
              <span>
                <strong>{thread.agent_name}</strong>
                <small>{thread.task}</small>
              </span>
              <em>{statusLabel(thread.status)}</em>
            </button>
          ))}
        </div>
      ) : null}
    </section>
  );
}
