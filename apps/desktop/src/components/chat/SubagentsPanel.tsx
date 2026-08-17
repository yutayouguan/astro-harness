import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Bot, RefreshCw, SendHorizontal, Square, X } from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";
import type { SubagentThread, SubagentThreadStatus } from "../../hooks/chat/useSubagentThreads";

type ThreadMessage = {
  id: number;
  role: string;
  content: string;
  created_at: string;
};

type ThreadDetail = {
  thread: SubagentThread;
  messages: ThreadMessage[];
};

type Props = {
  open: boolean;
  parentSessionId?: string | null;
  threads: SubagentThread[];
  initialThreadId?: string | null;
  refreshThreads: () => Promise<void>;
  onClose: () => void;
};

const ACTIVE_STATUSES = new Set<SubagentThreadStatus>(["pending", "running"]);

export default function SubagentsPanel({
  open,
  parentSessionId,
  threads,
  initialThreadId,
  refreshThreads,
  onClose,
}: Props) {
  const { t } = useI18n();
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [detail, setDetail] = useState<ThreadDetail | null>(null);
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const loadDetail = useCallback(async (threadId: string) => {
    if (!parentSessionId) return;
    try {
      const next = await invoke<ThreadDetail>("read_subagent_thread", {
        args: { parentSessionId, threadId },
      });
      setDetail(next);
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }, [parentSessionId]);

  useEffect(() => {
    if (!open) return;
    if (initialThreadId) setSelectedId(initialThreadId);
  }, [open, initialThreadId]);

  useEffect(() => {
    if (!open) return;
    setSelectedId((current) => {
      return current && threads.some((thread) => thread.id === current)
        ? current
        : threads[0]?.id ?? null;
    });
  }, [open, threads]);

  useEffect(() => {
    if (!open || !selectedId) {
      setDetail(null);
      return;
    }
    void loadDetail(selectedId);
    const timer = window.setInterval(() => void loadDetail(selectedId), 1500);
    return () => window.clearInterval(timer);
  }, [open, selectedId, loadDetail]);

  const selected = useMemo(
    () => threads.find((thread) => thread.id === selectedId) ?? detail?.thread ?? null,
    [threads, selectedId, detail],
  );
  const statusLabels: Record<SubagentThreadStatus, string> = {
    pending: t("subagents.status.pending"),
    running: t("subagents.status.running"),
    completed: t("subagents.status.completed"),
    failed: t("subagents.status.failed"),
    interrupted: t("subagents.status.interrupted"),
    closed: t("subagents.status.closed"),
  };

  const runAction = async (command: string, args: Record<string, unknown>) => {
    if (!parentSessionId) return;
    setBusy(true);
    try {
      await invoke(command, { args: { parentSessionId, ...args } });
      await refreshThreads();
      if (selectedId) await loadDetail(selectedId);
      setError(null);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  };

  const sendFollowUp = async () => {
    const text = message.trim();
    if (!selectedId || !text) return;
    setMessage("");
    await runAction("send_subagent_message", { threadId: selectedId, message: text });
  };

  if (!open) return null;

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

        {!parentSessionId ? (
          <div className="subagents-empty">{t("subagents.noSession")}</div>
        ) : threads.length === 0 ? (
          <div className="subagents-empty">{t("subagents.empty")}</div>
        ) : (
          <div className="subagents-layout">
            <nav className="subagents-list" aria-label={t("subagents.threadList")}>
              {threads.map((thread) => (
                <button
                  type="button"
                  key={thread.id}
                  className={thread.id === selectedId ? "is-active" : ""}
                  onClick={() => setSelectedId(thread.id)}
                >
                  <span className={`subagents-status is-${thread.status}`} />
                  <span className="subagents-list-copy">
                    <strong>{thread.agent_name}</strong>
                    <small>{thread.task}</small>
                  </span>
                </button>
              ))}
            </nav>

            <section className="subagents-detail">
              {selected ? (
                <div className="subagents-thread-head">
                  <div>
                    <strong>{selected.agent_name}</strong>
                    <span>{statusLabels[selected.status]}</span>
                  </div>
                  <div>
                    {ACTIVE_STATUSES.has(selected.status) ? (
                      <button
                        type="button"
                        disabled={busy}
                        onClick={() => void runAction("interrupt_subagent_thread", { threadId: selected.id })}
                        title={t("subagents.interrupt")}
                      ><Square size={13} />{t("subagents.interrupt")}</button>
                    ) : null}
                    {selected.status !== "closed" ? (
                      <button
                        type="button"
                        disabled={busy}
                        onClick={() => void runAction("close_subagent_thread", { threadId: selected.id })}
                        title={t("subagents.close")}
                      ><X size={14} />{t("subagents.close")}</button>
                    ) : null}
                  </div>
                </div>
              ) : null}

              <div className="subagents-messages">
                {detail?.messages.map((item) => (
                  <article key={item.id} className={`is-${item.role}`}>
                    <span>{item.role === "assistant" ? detail.thread.agent_name : t("subagents.parent")}</span>
                    <p>{item.content}</p>
                  </article>
                ))}
                {selected?.error ? <div className="subagents-error">{selected.error}</div> : null}
              </div>

              {selected && selected.status !== "closed" ? (
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
              ) : null}
            </section>
          </div>
        )}
        {error ? <div className="subagents-error subagents-error-global">{error}</div> : null}
      </aside>
    </div>
  );
}
