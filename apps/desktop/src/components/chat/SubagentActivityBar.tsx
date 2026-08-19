import { useMemo, useState, type CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Bot, ChevronDown, Square } from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";
import {
  flattenAgentTree,
  type AgentThreadStatus,
  type AgentTreeNode,
} from "../../hooks/chat/subagentTree";

type Props = {
  rootSessionId?: string | null;
  roots: AgentTreeNode[];
  onRefresh: () => Promise<void>;
  onOpenThread: (canonicalPath: string) => void;
  onOpenPanel: () => void;
};

function statusKey(status: AgentThreadStatus): string {
  return status.kind;
}

function visibleNode(node: AgentTreeNode): AgentTreeNode | null {
  if (node.archived) return null;
  const children = node.children
    .map(visibleNode)
    .filter((child): child is AgentTreeNode => child !== null);
  return { ...node, children };
}

export default function SubagentActivityBar({
  rootSessionId,
  roots,
  onRefresh,
  onOpenThread,
  onOpenPanel,
}: Props) {
  const { t } = useI18n();
  const [expanded, setExpanded] = useState(true);
  const [stopping, setStopping] = useState(false);
  const visibleRoots = useMemo(
    () => roots.map(visibleNode).filter((node): node is AgentTreeNode => node !== null),
    [roots],
  );
  const visible = useMemo(() => flattenAgentTree(visibleRoots), [visibleRoots]);
  const running = useMemo(
    () => visible.filter((node) => node.thread.status.kind === "running"),
    [visible],
  );

  if (!rootSessionId || visible.length === 0) return null;

  const stopAll = async () => {
    if (running.length === 0 || stopping) return;
    setStopping(true);
    try {
      await Promise.allSettled(
        running.map(({ thread }) =>
          invoke("interrupt_subagent_thread", {
            args: {
              rootSessionId,
              target: thread.canonicalPath,
            },
          })),
      );
      await onRefresh();
    } finally {
      setStopping(false);
    }
  };

  const renderNode = (node: AgentTreeNode, depth: number) => {
    const status = statusKey(node.thread.status);
    return (
      <div className="subagent-activity-branch" key={node.thread.threadId}>
        <button
          type="button"
          className={`is-${status}${node.unread ? " has-unread" : ""}`}
          style={{ "--depth": depth } as CSSProperties}
          onClick={() => onOpenThread(node.thread.canonicalPath)}
        >
          <span className={`subagents-status is-${status}`} aria-hidden />
          <span>
            <strong>{node.thread.taskName}</strong>
            <small>{node.thread.agentType}</small>
          </span>
          {node.unread ? (
            <span className="subagents-unread" role="img" aria-label={t("subagents.unread")} />
          ) : null}
          <em>{t(`subagents.status.${status}` as never)}</em>
        </button>
        {node.children.map((child) => renderNode(child, depth + 1))}
      </div>
    );
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
              {running.length > 0
                ? t("subagents.activity.running", { count: String(running.length) })
                : t("subagents.activity.done")}
            </small>
          </span>
          <ChevronDown className={expanded ? "is-open" : ""} size={14} aria-hidden />
        </button>
        <span className="subagent-activity-actions">
          {running.length > 0 ? (
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
          {visibleRoots.map((node) => renderNode(node, 0))}
        </div>
      ) : null}
    </section>
  );
}
