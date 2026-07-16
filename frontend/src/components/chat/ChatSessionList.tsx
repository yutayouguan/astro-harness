/** 近期会话列表：页签、菜单、归档与永久删除。 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { MoreHorizontal, Plus } from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";
import { useAgentsChanged } from "../../lib/agent/agentsChanged";
import {
  deleteManagedSession,
  dispatchSessionsChanged,
  subscribeSessionsChanged,
  type SessionListKind,
} from "../../lib/chat/sessionManagement";
import type { RecentSessionDto } from "../../types";
import type { AgentInfo } from "../../types/agent";
import { normalizeAgentId } from "../../types/agent";
import AgentPicker from "../agents/AgentPicker";
import ExpandableSearch from "../ui/ExpandableSearch";

/** 近期会话列表入参 */
type Props = {
  /** 当前打开的会话（高亮） */
  activeSessionId: string | null;
  onOpenSession: (sessionId: string) => void;
  /** 新建空白会话 */
  onNewSession: () => void;
  /** 新建 Agent 引导 */
  onNewAgent: () => void;
  /** 删除当前会话前取消流 */
  onPrepareDeleteCurrentSession?: () => void | Promise<void>;
  /** 删除当前会话后清理本地状态 */
  onClearDeletedCurrentSession?: () => void | Promise<void>;
};

function sessionTitle(
  item: RecentSessionDto,
  untitled: string,
): string {
  const summary = (item.summary ?? "").trim();
  return summary || untitled;
}

export default function ChatSessionList({
  activeSessionId,
  onOpenSession,
  onNewSession,
  onNewAgent,
  onPrepareDeleteCurrentSession,
  onClearDeletedCurrentSession,
}: Props) {
  const { t } = useI18n();
  const [items, setItems] = useState<RecentSessionDto[]>([]);
  const [query, setQuery] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [activeAgentId, setActiveAgentId] = useState("workspace");
  const [listKind, setListKind] = useState<SessionListKind>("active");
  const [menuSessionId, setMenuSessionId] = useState<string | null>(null);
  const [busySessionId, setBusySessionId] = useState<string | null>(null);
  const menuRef = useRef<HTMLDivElement | null>(null);

  const loadSessions = useCallback(async () => {
    try {
      const list = await invoke<RecentSessionDto[]>("list_sessions", {
        filter: listKind,
        limit: 50,
      });
      setItems(list ?? []);
      setError(null);
    } catch (e) {
      setError(String(e));
      setItems([]);
    }
  }, [listKind]);

  const loadAgents = useCallback(async () => {
    try {
      const cfg = await invoke<{
        active_agent_id: string;
        agents: AgentInfo[];
      }>("get_config");
      setAgents(cfg.agents ?? []);
      setActiveAgentId(normalizeAgentId(cfg.active_agent_id));
    } catch {
      setAgents([]);
    }
  }, []);

  useEffect(() => {
    void loadSessions();
    void loadAgents();
  }, [loadSessions, loadAgents]);

  useEffect(() => {
    const unlisten = subscribeSessionsChanged(() => {
      void loadSessions();
    });
    return () => unlisten();
  }, [loadSessions]);

  useEffect(() => {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    type SessionEventPayload = {
      sessionId?: string | null;
      sessionMetadataChanged?: { title: string } | null;
    };
    void listen<SessionEventPayload>("session_event", (ev) => {
      const sid = ev.payload.sessionId?.trim();
      const title = ev.payload.sessionMetadataChanged?.title?.trim();
      if (!sid || !title) return;
      setItems((prev) =>
        prev.map((item) =>
          item.sessionId === sid ? { ...item, summary: title } : item,
        ),
      );
    })
      .then((fn) => {
        if (disposed) fn();
        else unlisten = fn;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (!menuSessionId) return;
    const onPointerDown = (event: MouseEvent) => {
      if (menuRef.current?.contains(event.target as Node)) return;
      setMenuSessionId(null);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setMenuSessionId(null);
    };
    window.addEventListener("mousedown", onPointerDown);
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("mousedown", onPointerDown);
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [menuSessionId]);

  useAgentsChanged((payload) => {
    setActiveAgentId(normalizeAgentId(payload.active_agent_id));
    void loadAgents();
  });

  const handleAgentChange = useCallback(async (id: string) => {
    try {
      const cfg = await invoke<{
        active_agent_id: string;
        agents: AgentInfo[];
      }>("set_active_agent", { agentId: id });
      setAgents(cfg.agents ?? []);
      setActiveAgentId(normalizeAgentId(cfg.active_agent_id));
    } catch (e) {
      console.warn("set_active_agent failed", e);
    }
  }, []);

  const runSessionAction = useCallback(
    async (sessionId: string, action: () => Promise<void>) => {
      setBusySessionId(sessionId);
      setMenuSessionId(null);
      setError(null);
      try {
        await action();
        dispatchSessionsChanged();
      } catch (e) {
        setError(
          t("sessions.actionFailed", {
            error: e instanceof Error ? e.message : String(e),
          }),
        );
      } finally {
        setBusySessionId(null);
      }
    },
    [t],
  );

  const handleRename = useCallback(
    (item: RecentSessionDto) => {
      const untitled = t("chat.rightPanel.untitledSession");
      const current = sessionTitle(item, untitled);
      const next = window.prompt(t("sessions.renamePrompt"), current);
      if (next === null) return;
      const title = next.trim();
      if (!title) return;
      void runSessionAction(item.sessionId, async () => {
        await invoke("rename_session", {
          sessionId: item.sessionId,
          title,
        });
      });
    },
    [runSessionAction, t],
  );

  const handleRegenerateTitle = useCallback(
    (item: RecentSessionDto) => {
      void runSessionAction(item.sessionId, async () => {
        const title = await invoke<string>("regenerate_session_title", {
          sessionId: item.sessionId,
        });
        const next = title.trim();
        if (!next) return;
        setItems((prev) =>
          prev.map((row) =>
            row.sessionId === item.sessionId ? { ...row, summary: next } : row,
          ),
        );
      });
    },
    [runSessionAction],
  );

  const handleArchiveToggle = useCallback(
    (item: RecentSessionDto) => {
      const command =
        listKind === "archived" ? "unarchive_session" : "archive_session";
      void runSessionAction(item.sessionId, async () => {
        await invoke(command, { sessionId: item.sessionId });
      });
    },
    [listKind, runSessionAction],
  );

  const handleDelete = useCallback(
    (item: RecentSessionDto) => {
      const untitled = t("chat.rightPanel.untitledSession");
      const title = sessionTitle(item, untitled);
      const confirmed = window.confirm(
        t("sessions.deleteConfirm", { title }),
      );
      if (!confirmed) {
        setMenuSessionId(null);
        return;
      }
      void runSessionAction(item.sessionId, async () => {
        if (item.sessionId === activeSessionId) {
          await onPrepareDeleteCurrentSession?.();
        }
        await deleteManagedSession(
          item.sessionId,
          activeSessionId,
          async () => {
            await invoke("delete_session_permanently", {
              sessionId: item.sessionId,
            });
          },
          async () => {
            await onClearDeletedCurrentSession?.();
          },
        );
      });
    },
    [
      activeSessionId,
      onPrepareDeleteCurrentSession,
      onClearDeletedCurrentSession,
      runSessionAction,
      t,
    ],
  );

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return items;
    return items.filter(
      (s) =>
        (s.summary ?? "").toLowerCase().includes(q) ||
        s.sessionId.toLowerCase().includes(q),
    );
  }, [items, query]);

  const emptyLabel =
    listKind === "archived"
      ? t("sessions.noArchived")
      : t("chat.rightPanel.noSessions");

  return (
    <div className="chat-session-list">
      <div className="chat-session-tabs" role="tablist" aria-label={t("sessions.tabs")}>
        <button
          type="button"
          role="tab"
          aria-selected={listKind === "active"}
          className={`chat-session-tab ${listKind === "active" ? "is-active" : ""}`}
          onClick={() => {
            setListKind("active");
            setMenuSessionId(null);
            setQuery("");
          }}
        >
          {t("sessions.active")}
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={listKind === "archived"}
          className={`chat-session-tab ${listKind === "archived" ? "is-active" : ""}`}
          onClick={() => {
            setListKind("archived");
            setMenuSessionId(null);
            setQuery("");
          }}
        >
          {t("sessions.archived")}
        </button>
      </div>

      <div className="chat-session-toolbar">
        <ExpandableSearch
          value={query}
          onChange={setQuery}
          placeholderKey="chat.rightPanel.searchSessions"
          className="chat-session-search"
        />
        <button
          type="button"
          className="chat-session-new"
          onClick={onNewSession}
        >
          <Plus size={15} strokeWidth={2.2} aria-hidden />
          {t("chat.newSession")}
        </button>
        <AgentPicker
          agents={agents}
          value={activeAgentId}
          onChange={(id) => void handleAgentChange(id)}
          onCreateNew={onNewAgent}
          labelKey="chat.rightPanel.agent"
          className="chat-session-agent-picker"
        />
      </div>
      {error && <div className="side-error">{error}</div>}
      {filtered.length === 0 ? (
        <p className="muted">{emptyLabel}</p>
      ) : (
        <ul>
          {filtered.map((s) => {
            const title = sessionTitle(s, t("chat.rightPanel.untitledSession"));
            const busy = busySessionId === s.sessionId;
            const menuOpen = menuSessionId === s.sessionId;
            return (
              <li key={s.sessionId} className="chat-session-row">
                <button
                  type="button"
                  className={`chat-session-item ${
                    s.sessionId === activeSessionId ? "is-active" : ""
                  }`}
                  onClick={() => onOpenSession(s.sessionId)}
                  disabled={busy}
                >
                  <strong>
                    {title}
                    {s.endReason === "compacted" ? (
                      <span className="chat-session-badge">
                        {t("chat.sessionCompactedBadge")}
                      </span>
                    ) : null}
                  </strong>
                  <span>{s.sessionId.slice(0, 8)}</span>
                </button>
                <div
                  className={`chat-session-menu-wrap ${menuOpen ? "is-open" : ""}`}
                  ref={menuOpen ? menuRef : null}
                >
                  <button
                    type="button"
                    className="chat-session-more"
                    aria-label={t("sessions.moreActions")}
                    aria-haspopup="menu"
                    aria-expanded={menuOpen}
                    disabled={busy}
                    onClick={(e) => {
                      e.stopPropagation();
                      setMenuSessionId(menuOpen ? null : s.sessionId);
                    }}
                  >
                    <MoreHorizontal size={16} strokeWidth={2.2} aria-hidden />
                  </button>
                  {menuOpen ? (
                    <div className="chat-session-menu" role="menu">
                      <button
                        type="button"
                        role="menuitem"
                        disabled={busy}
                        onClick={() => handleRename(s)}
                      >
                        {t("sessions.rename")}
                      </button>
                      <button
                        type="button"
                        role="menuitem"
                        disabled={busy}
                        onClick={() => handleRegenerateTitle(s)}
                      >
                        {t("sessions.regenerateTitle")}
                      </button>
                      <button
                        type="button"
                        role="menuitem"
                        disabled={busy}
                        onClick={() => handleArchiveToggle(s)}
                      >
                        {listKind === "archived"
                          ? t("sessions.unarchive")
                          : t("sessions.archive")}
                      </button>
                      <button
                        type="button"
                        role="menuitem"
                        className="is-danger"
                        disabled={busy}
                        onClick={() => handleDelete(s)}
                      >
                        {t("sessions.deletePermanently")}
                      </button>
                    </div>
                  ) : null}
                </div>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
