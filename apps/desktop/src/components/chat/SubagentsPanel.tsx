import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Bot, RefreshCw, SendHorizontal, Square, Wrench, X } from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";
import {
  normalizeAgentThreadDetail,
  type AgentThread,
  type AgentThreadDetail,
  type AgentThreadMessage,
  type AgentTreeNode,
} from "../../hooks/chat/subagentTree";

type Props = {
  open: boolean;
  rootSessionId?: string | null;
  roots: AgentTreeNode[];
  threads: AgentThread[];
  initialTarget?: string | null;
  streamError?: string | null;
  loading: boolean;
  initialized: boolean;
  refreshThreads: () => Promise<void>;
  markRead: (canonicalPath: string) => void;
  onClose: () => void;
};

type FlatNode = { node: AgentTreeNode; depth: number };

function flattenWithDepth(nodes: readonly AgentTreeNode[], depth = 0): FlatNode[] {
  return nodes.flatMap((node) => [
    { node, depth },
    ...flattenWithDepth(node.children, depth + 1),
  ]);
}

function messageText(message: AgentThreadMessage): string {
  return message.content
    ?? message.compressedContent
    ?? message.reasoningContent
    ?? message.reasoning
    ?? "";
}

function toolMetadata(message: AgentThreadMessage): string | null {
  const metadata = {
    ...(message.toolName ? { tool: message.toolName } : {}),
    ...(message.toolCallId ? { callId: message.toolCallId } : {}),
    ...(message.toolCalls ? { calls: message.toolCalls } : {}),
  };
  return Object.keys(metadata).length > 0 ? JSON.stringify(metadata, null, 2) : null;
}

export default function SubagentsPanel({
  open,
  rootSessionId,
  roots,
  threads,
  initialTarget,
  streamError,
  loading,
  initialized,
  refreshThreads,
  markRead,
  onClose,
}: Props) {
  const { t } = useI18n();
  const root = rootSessionId?.trim() ?? "";
  const activeRootRef = useRef(root);
  const generationRef = useRef(0);
  if (activeRootRef.current !== root) {
    activeRootRef.current = root;
    generationRef.current += 1;
  }
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [detail, setDetail] = useState<AgentThreadDetail | null>(null);
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  const [loadingDetail, setLoadingDetail] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const detailRequestRef = useRef(0);
  const actionRequestRef = useRef(0);
  const flatNodes = useMemo(() => flattenWithDepth(roots), [roots]);

  const selected = useMemo(
    () => threads.find((thread) => thread.canonicalPath === selectedPath)
      ?? (detail?.thread.canonicalPath === selectedPath ? detail.thread : null),
    [threads, selectedPath, detail],
  );

  const loadDetail = useCallback(async (canonicalPath: string) => {
    if (!root) return;
    const generation = generationRef.current;
    const isCurrentRoot = () => activeRootRef.current === root
      && generationRef.current === generation;
    if (!isCurrentRoot()) return;
    const request = ++detailRequestRef.current;
    setDetail((current) =>
      current?.thread.canonicalPath === canonicalPath ? current : null);
    setLoadingDetail(true);
    try {
      const raw = await invoke<unknown>("read_subagent_thread", {
        args: { rootSessionId: root, target: canonicalPath },
      });
      if (!isCurrentRoot() || request !== detailRequestRef.current) return;
      const next = normalizeAgentThreadDetail(raw);
      if (next.thread.canonicalPath !== canonicalPath) {
        throw new Error("agent thread detail belongs to another target");
      }
      setDetail(next);
      setError(null);
    } catch (reason) {
      if (!isCurrentRoot() || request !== detailRequestRef.current) return;
      setError(String(reason));
    } finally {
      if (isCurrentRoot() && request === detailRequestRef.current) {
        setLoadingDetail(false);
      }
    }
  }, [root]);

  useEffect(() => {
    const generation = generationRef.current;
    detailRequestRef.current += 1;
    actionRequestRef.current += 1;
    setSelectedPath(null);
    setDetail(null);
    setMessage("");
    setBusy(false);
    setLoadingDetail(false);
    setError(null);
    return () => {
      if (activeRootRef.current === root && generationRef.current === generation) {
        generationRef.current += 1;
      }
      detailRequestRef.current += 1;
      actionRequestRef.current += 1;
    };
  }, [root]);

  useEffect(() => {
    if (!open || !initialTarget) return;
    setSelectedPath(initialTarget);
  }, [open, initialTarget]);

  useEffect(() => {
    if (!open) return;
    setSelectedPath((current) =>
      current && threads.some((thread) => thread.canonicalPath === current)
        ? current
        : threads[0]?.canonicalPath ?? null);
  }, [open, threads]);

  useEffect(() => {
    if (!open || !selected) {
      detailRequestRef.current += 1;
      setDetail(null);
      setLoadingDetail(false);
      return;
    }
    markRead(selected.canonicalPath);
    void loadDetail(selected.canonicalPath);
    return () => {
      detailRequestRef.current += 1;
    };
    // `selected` retains object identity for unrelated Agent Thread events, so
    // this reloads only on selection or an event/snapshot for this target.
  }, [open, selected, loadDetail, markRead]);

  const runAction = async (
    command: string,
    target: string,
    extra: Record<string, unknown> = {},
  ): Promise<boolean> => {
    if (!root) return false;
    const generation = generationRef.current;
    const actionRequest = ++actionRequestRef.current;
    const isCurrentAction = () => activeRootRef.current === root
      && generationRef.current === generation
      && actionRequest === actionRequestRef.current;
    if (!isCurrentAction()) return false;
    setBusy(true);
    try {
      await invoke(command, { args: { rootSessionId: root, target, ...extra } });
      if (!isCurrentAction()) return false;
      await refreshThreads();
      if (!isCurrentAction()) return false;
      setError(null);
      return true;
    } catch (reason) {
      if (!isCurrentAction()) return false;
      setError(String(reason));
      return false;
    } finally {
      if (isCurrentAction()) setBusy(false);
    }
  };

  const sendFollowUp = async () => {
    const text = message.trim();
    if (!selected || !text) return;
    const sent = await runAction(
      "send_subagent_message",
      selected.canonicalPath,
      { message: text },
    );
    if (sent) setMessage("");
  };

  if (!open) return null;
  const status = selected?.status.kind;
  const archived = status === "shutdown";

  return (
    <div className="subagents-backdrop" role="presentation" onMouseDown={onClose}>
      <aside
        className="subagents-panel"
        aria-label={t("subagents.title")}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="subagents-head">
          <div>
            <h2><Bot size={17} />{t("subagents.title")}</h2>
            <p>{t("subagents.subtitle")}</p>
          </div>
          <div className="subagents-head-actions">
            <button type="button" onClick={() => void refreshThreads()} aria-label={t("subagents.refresh")}>
              <RefreshCw size={15} />
            </button>
            <button type="button" onClick={onClose} aria-label={t("subagents.closePanel")}>
              <X size={16} />
            </button>
          </div>
        </header>

        {!root ? (
          <div className="subagents-empty">{t("subagents.noSession")}</div>
        ) : !initialized && streamError ? (
          <div className="subagents-empty">{t("subagents.loadFailed")}</div>
        ) : !initialized || (loading && threads.length === 0) ? (
          <div className="subagents-empty">{t("subagents.loading")}</div>
        ) : threads.length === 0 ? (
          <div className="subagents-empty">{t("subagents.empty")}</div>
        ) : (
          <div className="subagents-layout">
            <nav className="subagents-list" role="tree" aria-label={t("subagents.threadList")}>
              {flatNodes.map(({ node, depth }) => {
                const thread = node.thread;
                const kind = thread.status.kind;
                return (
                  <button
                    type="button"
                    role="treeitem"
                    aria-level={depth + 1}
                    key={thread.threadId}
                    className={`${thread.canonicalPath === selectedPath ? "is-active" : ""}${node.archived ? " is-archived" : ""}`.trim()}
                    style={{ "--depth": depth } as CSSProperties}
                    onClick={() => {
                      setSelectedPath(thread.canonicalPath);
                      markRead(thread.canonicalPath);
                    }}
                  >
                    <span className={`subagents-status is-${kind}`} />
                    <span className="subagents-list-copy">
                      <strong>{thread.taskName}</strong>
                      <small>{thread.agentType}</small>
                    </span>
                    {node.unread ? (
                      <span className="subagents-unread" role="img" aria-label={t("subagents.unread")} />
                    ) : null}
                  </button>
                );
              })}
            </nav>

            <section className="subagents-detail">
              {selected ? (
                <div className="subagents-thread-head">
                  <div>
                    <strong>{selected.taskName}</strong>
                    <span>{t(`subagents.status.${status}` as never)}</span>
                  </div>
                  <div>
                    {status === "running" ? (
                      <button
                        type="button"
                        disabled={busy}
                        onClick={() => void runAction("interrupt_subagent_thread", selected.canonicalPath)}
                        title={t("subagents.interrupt")}
                      ><Square size={13} />{t("subagents.interrupt")}</button>
                    ) : null}
                    {!archived ? (
                      <button
                        type="button"
                        disabled={busy}
                        onClick={() => void runAction("close_subagent_thread", selected.canonicalPath)}
                        title={t("subagents.close")}
                      ><X size={14} />{t("subagents.close")}</button>
                    ) : null}
                  </div>
                </div>
              ) : null}

              <div className="subagents-messages" aria-busy={loadingDetail}>
                {loadingDetail && !detail ? (
                  <div className="subagents-detail-state">{t("subagents.loading")}</div>
                ) : null}
                {detail?.messages.map((item) => {
                  const metadata = toolMetadata(item);
                  const isTool = item.role === "tool" || Boolean(item.toolName);
                  return (
                    <article key={item.id} className={`is-${item.role}`}>
                      <span>
                        {isTool ? <Wrench size={11} aria-hidden /> : null}
                        {isTool
                          ? item.toolName ?? t("subagents.tool")
                          : item.role === "assistant"
                            ? detail.thread.taskName
                            : t("subagents.parent")}
                      </span>
                      {messageText(item) ? <p>{messageText(item)}</p> : null}
                      {metadata ? <pre>{metadata}</pre> : null}
                    </article>
                  );
                })}
                {detail && detail.messages.length === 0 && !loadingDetail ? (
                  <div className="subagents-detail-state">{t("subagents.noMessages")}</div>
                ) : null}
                {status === "errored" && selected?.status.kind === "errored" ? (
                  <div className="subagents-error">{selected.status.payload.message}</div>
                ) : null}
              </div>

              {selected && !archived ? (
                <form
                  className="subagents-composer"
                  onSubmit={(event) => {
                    event.preventDefault();
                    void sendFollowUp();
                  }}
                >
                  <textarea
                    value={message}
                    onChange={(event) => setMessage(event.target.value)}
                    placeholder={t("subagents.followUp")}
                    rows={2}
                  />
                  <button type="submit" disabled={busy || !message.trim()} aria-label={t("subagents.send")}>
                    <SendHorizontal size={15} />
                  </button>
                </form>
              ) : archived ? (
                <div className="subagents-archive-note">{t("subagents.archiveReadOnly")}</div>
              ) : null}
            </section>
          </div>
        )}
        {error || streamError ? (
          <div className="subagents-error subagents-error-global">{error ?? streamError}</div>
        ) : null}
      </aside>
    </div>
  );
}
