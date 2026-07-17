/** 近期会话列表：页签、菜单、归档与永久删除。 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  Archive,
  ArchiveRestore,
  Download,
  GitBranch,
  LoaderCircle,
  MoreVertical,
  Pencil,
  Pin,
  PinOff,
  Plus,
  RefreshCw,
  Trash2,
} from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";
import type { MessageKey } from "../../i18n/messages";
import { useAgentsChanged } from "../../lib/agent/agentsChanged";
import {
  deleteManagedSession,
  dispatchSessionsChanged,
  subscribeSessionsChanged,
  type SessionListKind,
} from "../../lib/chat/sessionManagement";
import {
  clearSessionUnread,
  isSessionUnread,
  markSessionUnread,
  subscribeSessionUnread,
} from "../../lib/chat/sessionUnread";
import { useTransientToast } from "../../hooks/ui/useTransientToast";
import type { RecentSessionDto } from "../../types";
import type { AgentInfo } from "../../types/agent";
import { normalizeAgentId } from "../../types/agent";
import AgentPicker from "../agents/AgentPicker";
import ExpandableSearch from "../ui/ExpandableSearch";
import EmptyIllustration from "../../illustrations/EmptyIllustration";

type ChatHistoryExportDto = {
  messages: Array<{
    role: string;
    content: string;
  }>;
};

function sessionTitle(
  item: RecentSessionDto,
  untitled: string,
): string {
  const summary = (item.summary ?? "").trim();
  return summary || untitled;
}

function isPinned(item: RecentSessionDto): boolean {
  return Boolean(item.pinnedAt);
}

/** RFC3339 / ISO → 相对时间（1 分钟前 / 2 小时前 …） */
function formatSessionRelativeTime(
  iso: string | null | undefined,
  t: (key: MessageKey, vars?: Record<string, string>) => string,
): string {
  if (!iso) return t("time.justNow");
  const ms = Date.parse(iso);
  if (!Number.isFinite(ms)) return t("time.justNow");
  const diff = Math.max(0, Date.now() - ms);
  const minute = 60_000;
  const hour = 60 * minute;
  const day = 24 * hour;
  const week = 7 * day;
  const month = 30 * day;
  const year = 365 * day;

  if (diff < minute) return t("time.justNow");
  if (diff < hour) {
    return t("time.minutesAgo", { n: String(Math.floor(diff / minute)) });
  }
  if (diff < day) {
    return t("time.hoursAgo", { n: String(Math.floor(diff / hour)) });
  }
  if (diff < week) {
    return t("time.daysAgo", { n: String(Math.floor(diff / day)) });
  }
  if (diff < month) {
    return t("time.weeksAgo", { n: String(Math.floor(diff / week)) });
  }
  if (diff < year) {
    return t("time.monthsAgo", { n: String(Math.floor(diff / month)) });
  }
  return t("time.yearsAgo", { n: String(Math.max(1, Math.floor(diff / year))) });
}

function sanitizeExportFilename(title: string, sessionId: string): string {
  const base = title
    .replace(/[\\/:*?"<>|]+/g, "_")
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, 48);
  const id = sessionId.slice(0, 8);
  return `${base || "session"}-${id}.md`;
}

function utf8ToBase64(text: string): string {
  const bytes = new TextEncoder().encode(text);
  let bin = "";
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {
    bin += String.fromCharCode(...bytes.subarray(i, i + chunk));
  }
  return btoa(bin);
}

function historyToMarkdown(
  title: string,
  sessionId: string,
  messages: ChatHistoryExportDto["messages"],
): string {
  const lines = [
    `# ${title}`,
    "",
    `> session: \`${sessionId}\``,
    "",
  ];
  for (const msg of messages) {
    const role = msg.role.trim() || "message";
    const content = (msg.content ?? "").trim();
    if (!content) continue;
    lines.push(`## ${role}`, "", content, "");
  }
  return `${lines.join("\n").trim()}\n`;
}

/** 近期会话列表入参 */
type Props = {
  /** 当前打开的会话（高亮） */
  activeSessionId: string | null;
  /** 正在流式输出的会话；无流式时为 null */
  streamingSessionId?: string | null;
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

export default function ChatSessionList({
  activeSessionId,
  streamingSessionId = null,
  onOpenSession,
  onNewSession,
  onNewAgent,
  onPrepareDeleteCurrentSession,
  onClearDeletedCurrentSession,
}: Props) {
  const { t } = useI18n();
  const { showToast, toastHost } = useTransientToast();
  const [items, setItems] = useState<RecentSessionDto[]>([]);
  const [query, setQuery] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [activeAgentId, setActiveAgentId] = useState("workspace");
  const [listKind, setListKind] = useState<SessionListKind>("active");
  const [menuSessionId, setMenuSessionId] = useState<string | null>(null);
  const [busySessionId, setBusySessionId] = useState<string | null>(null);
  const [unreadTick, setUnreadTick] = useState(0);
  const menuRef = useRef<HTMLDivElement | null>(null);
  const prevStreamingRef = useRef<string | null>(null);

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

  useEffect(() => subscribeSessionUnread(() => setUnreadTick((n) => n + 1)), []);

  // 流式结束 → 标未读；working 中用动画图标。只有用户点击进入会话才清未读。
  useEffect(() => {
    const prev = prevStreamingRef.current;
    prevStreamingRef.current = streamingSessionId;
    if (prev && !streamingSessionId) {
      markSessionUnread(prev);
      setUnreadTick((n) => n + 1);
    }
  }, [streamingSessionId]);

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

  const handlePinToggle = useCallback(
    (item: RecentSessionDto) => {
      const command = isPinned(item) ? "unpin_session" : "pin_session";
      void runSessionAction(item.sessionId, async () => {
        await invoke(command, { sessionId: item.sessionId });
      });
    },
    [runSessionAction],
  );

  const handleExport = useCallback(
    (item: RecentSessionDto) => {
      void runSessionAction(item.sessionId, async () => {
        const history = await invoke<ChatHistoryExportDto>("get_chat_history", {
          sessionId: item.sessionId,
          limit: 500,
        });
        const untitled = t("chat.rightPanel.untitledSession");
        const title = sessionTitle(item, untitled);
        const markdown = historyToMarkdown(
          title,
          item.sessionId,
          history.messages ?? [],
        );
        if (!markdown.replace(/^#.*$/m, "").trim()) {
          throw new Error(t("sessions.exportEmpty"));
        }
        const savedPath = await invoke<string>("download_bytes_to_downloads", {
          filename: sanitizeExportFilename(title, item.sessionId),
          base64Data: utf8ToBase64(markdown),
        });
        showToast(t("sessions.exportDone", { path: savedPath }), {
          tone: "success",
        });
      });
    },
    [runSessionAction, showToast, t],
  );

  const handleBranch = useCallback(
    (item: RecentSessionDto) => {
      void runSessionAction(item.sessionId, async () => {
        const history = await invoke<ChatHistoryExportDto>("get_chat_history", {
          sessionId: item.sessionId,
          limit: 500,
        });
        const bubbles = (history.messages ?? []).filter((m) => {
          const role = m.role.trim().toLowerCase();
          return role === "user" || role === "assistant";
        });
        if (bubbles.length === 0) {
          throw new Error(t("sessions.branchEmpty"));
        }
        const newId = await invoke<string>("fork_chat_session", {
          sourceSessionId: item.sessionId,
          keepChatBubbles: bubbles.length,
        });
        onOpenSession(newId);
      });
    },
    [onOpenSession, runSessionAction, t],
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

  const hasQuery = query.trim().length > 0;
  const emptyTitle = hasQuery
    ? t("sessions.searchEmpty")
    : listKind === "archived"
      ? t("sessions.noArchived")
      : t("chat.rightPanel.noSessions");
  const emptyHint = hasQuery
    ? undefined
    : listKind === "archived"
      ? t("sessions.noArchivedHint")
      : t("sessions.emptyHint");

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
          <Plus size={15} strokeWidth={1.75} aria-hidden />
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
        <EmptyIllustration
          scene="chat"
          size="sm"
          className="chat-session-empty"
          title={emptyTitle}
          hint={emptyHint}
        />
      ) : (
        <ul>
          {filtered.map((s) => {
            const title = sessionTitle(s, t("chat.rightPanel.untitledSession"));
            const busy = busySessionId === s.sessionId;
            const menuOpen = menuSessionId === s.sessionId;
            const inProgress = streamingSessionId === s.sessionId;
            const unread = !inProgress && isSessionUnread(s.sessionId);
            void unreadTick;
            const pinned = isPinned(s);
            return (
              <li
                key={s.sessionId}
                className={`chat-session-row ${
                  s.sessionId === activeSessionId ? "is-active" : ""
                } ${menuOpen ? "is-menu-open" : ""} ${
                  inProgress ? "is-in-progress" : ""
                } ${unread ? "is-unread" : ""}`}
              >
                <button
                  type="button"
                  className="chat-session-item"
                  onClick={() => {
                    clearSessionUnread(s.sessionId);
                    setUnreadTick((n) => n + 1);
                    onOpenSession(s.sessionId);
                  }}
                  disabled={busy}
                >
                  <span className="chat-session-status" aria-hidden>
                    {inProgress ? (
                      <LoaderCircle
                        className="chat-session-status-spin"
                        size={14}
                        strokeWidth={2.2}
                      />
                    ) : unread ? (
                      <span className="chat-session-unread-dot" />
                    ) : (
                      <span className="chat-session-status-spacer" />
                    )}
                  </span>
                  <strong>
                    {title}
                    {pinned ? (
                      <span className="chat-session-badge is-pinned">
                        {t("sessions.pinnedBadge")}
                      </span>
                    ) : null}
                    {s.endReason === "compacted" ? (
                      <span className="chat-session-badge">
                        {t("chat.sessionCompactedBadge")}
                      </span>
                    ) : null}
                  </strong>
                  <span className="chat-session-time">
                    {formatSessionRelativeTime(s.createdAt, t)}
                  </span>
                </button>
                <div className="chat-session-quick">
                  <button
                    type="button"
                    className={`chat-session-quick-btn ${pinned ? "is-on" : ""}`}
                    title={pinned ? t("sessions.unpin") : t("sessions.pin")}
                    aria-label={pinned ? t("sessions.unpin") : t("sessions.pin")}
                    disabled={busy}
                    onClick={(e) => {
                      e.stopPropagation();
                      handlePinToggle(s);
                    }}
                  >
                    {pinned ? (
                      <PinOff size={14} strokeWidth={1.75} aria-hidden />
                    ) : (
                      <Pin size={14} strokeWidth={1.75} aria-hidden />
                    )}
                  </button>
                  <button
                    type="button"
                    className="chat-session-quick-btn"
                    title={
                      listKind === "archived"
                        ? t("sessions.unarchive")
                        : t("sessions.archive")
                    }
                    aria-label={
                      listKind === "archived"
                        ? t("sessions.unarchive")
                        : t("sessions.archive")
                    }
                    disabled={busy}
                    onClick={(e) => {
                      e.stopPropagation();
                      handleArchiveToggle(s);
                    }}
                  >
                    {listKind === "archived" ? (
                      <ArchiveRestore size={14} strokeWidth={1.75} aria-hidden />
                    ) : (
                      <Archive size={14} strokeWidth={1.75} aria-hidden />
                    )}
                  </button>
                </div>
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
                    <MoreVertical size={15} strokeWidth={1.75} aria-hidden />
                  </button>
                  {menuOpen ? (
                    <div className="chat-session-menu" role="menu">
                      <button
                        type="button"
                        role="menuitem"
                        disabled={busy}
                        onClick={() => handlePinToggle(s)}
                      >
                        {isPinned(s) ? (
                          <PinOff size={14} strokeWidth={1.75} aria-hidden />
                        ) : (
                          <Pin size={14} strokeWidth={1.75} aria-hidden />
                        )}
                        {isPinned(s) ? t("sessions.unpin") : t("sessions.pin")}
                      </button>
                      <button
                        type="button"
                        role="menuitem"
                        disabled={busy}
                        onClick={() => handleRename(s)}
                      >
                        <Pencil size={14} strokeWidth={1.75} aria-hidden />
                        {t("sessions.rename")}
                      </button>
                      <button
                        type="button"
                        role="menuitem"
                        disabled={busy}
                        onClick={() => handleRegenerateTitle(s)}
                      >
                        <RefreshCw size={14} strokeWidth={1.75} aria-hidden />
                        {t("sessions.regenerateTitle")}
                      </button>
                      <button
                        type="button"
                        role="menuitem"
                        disabled={busy}
                        onClick={() => handleExport(s)}
                      >
                        <Download size={14} strokeWidth={1.75} aria-hidden />
                        {t("sessions.export")}
                      </button>
                      <button
                        type="button"
                        role="menuitem"
                        disabled={busy}
                        onClick={() => handleBranch(s)}
                      >
                        <GitBranch size={14} strokeWidth={1.75} aria-hidden />
                        {t("sessions.branch")}
                      </button>
                      <button
                        type="button"
                        role="menuitem"
                        disabled={busy}
                        onClick={() => handleArchiveToggle(s)}
                      >
                        {listKind === "archived" ? (
                          <ArchiveRestore size={14} strokeWidth={1.75} aria-hidden />
                        ) : (
                          <Archive size={14} strokeWidth={1.75} aria-hidden />
                        )}
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
                        <Trash2 size={14} strokeWidth={1.75} aria-hidden />
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
      {toastHost}
    </div>
  );
}
