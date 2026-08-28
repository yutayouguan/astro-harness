// 侧栏会话列表：按项目分组或全局搜索结果，含全部会话操作。

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  Download,
  Edit3,
  GitBranch,
  MoreVertical,
  Pin,
  RefreshCw,
  Trash2,
} from "lucide-react";
import {
  Archive as ArchiveData,
  ArchiveRestore as ArchiveRestoreData,
  ChevronDown as ChevronDownData,
  ChevronUp as ChevronUpData,
  Pin as PinData,
  PinOff as PinOffData,
} from "lucide";
import { MorphToggleIcon } from "../icons/MorphIcon";
import {
  deleteManagedSession,
  dispatchSessionsChanged,
  subscribeSessionsChanged,
  type SessionListKind,
} from "../../lib/chat/sessionManagement";
import {
  clearSessionUnread,
  isSessionUnread,
  subscribeSessionUnread,
} from "../../lib/chat/sessionUnread";
import { useAppDialog } from "../../hooks/ui/DialogContext";
import { useTransientToast } from "../../hooks/ui/useTransientToast";
import { useI18n } from "../../i18n/LocaleContext";
import type { MessageKey } from "../../i18n/messages";
import type { RecentSessionDto } from "../../types";
import type { SessionStatusMap } from "../../hooks/chat/useSessionStatusMap";
import { visibleSessionTitle } from "../../lib/chat/sessionTitle";
import SessionStatusIcon, {
  resolveSessionStatus,
  type SessionActivityStatus,
} from "./SessionStatusIcon";

type Translate = (key: MessageKey, vars?: Record<string, string>) => string;

const DEFAULT_VISIBLE_COUNT = 5;
const STORAGE_KEY = "astro:sidebar-visible-sessions";
function readVisibleCount(): number {
  try {
    const v = localStorage.getItem(STORAGE_KEY);
    if (v) { const n = Number(v); if (n >= 1 && n <= 50) return n; }
  } catch {}
  return DEFAULT_VISIBLE_COUNT;
}

type ChatHistoryExportDto = {
  messages: Array<{ role: string; content: string }>;
};

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
  const lines = [`# ${title}`, "", `> session: \`${sessionId}\``, ""];
  for (const msg of messages) {
    const role = msg.role.trim() || "message";
    const content = (msg.content ?? "").trim();
    if (!content) continue;
    lines.push(`## ${role}`, "", content, "");
  }
  return `${lines.join("\n").trim()}\n`;
}

type Props = {
  activeSessionId: string | null;
  sessionStatuses: SessionStatusMap;
  /** null 表示跨项目的全局列表：不按项目过滤，也不自动关联会话 */
  projectId: string | null;
  /** 侧栏全局搜索词；非空时展示全部匹配结果 */
  query: string;
  listKind: SessionListKind;
  /** 按置顶状态过滤：pinned 只显示已置顶，unpinned 只显示未置顶，all 不过滤 */
  pinnedFilter?: "all" | "pinned" | "unpinned";
  /** 过滤后条目数变化时回调，用于外部按需隐藏整个分区 */
  onCountChange?: (count: number) => void;
  onOpenSession: (sessionId: string) => void;
  /** 删除当前会话前取消流 */
  onPrepareDeleteCurrentSession?: () => void | Promise<void>;
  /** 当前会话被删除后清理本地状态 */
  onClearDeletedCurrentSession?: () => void | Promise<void>;
};

function relativeTime(iso: string | null, t: Translate): string {
  if (!iso) return "";
  const diff = Date.now() - new Date(iso).getTime();
  const mins = Math.floor(diff / 60000);
  if (mins < 1) return t("time.justNow");
  if (mins < 60) return t("time.minutesAgo", { n: String(mins) });
  const hours = Math.floor(mins / 60);
  if (hours < 24) return t("time.hoursAgo", { n: String(hours) });
  return t("time.daysAgo", { n: String(Math.floor(hours / 24)) });
}

export default function SidebarSessionList({
  activeSessionId,
  sessionStatuses,
  projectId,
  query,
  listKind,
  pinnedFilter = "all",
  onCountChange,
  onOpenSession,
  onPrepareDeleteCurrentSession,
  onClearDeletedCurrentSession,
}: Props) {
  const { t } = useI18n();
  const { confirm, prompt } = useAppDialog();
  const { showToast, toastHost } = useTransientToast();
  const [items, setItems] = useState<RecentSessionDto[]>([]);
  const [expanded, setExpanded] = useState(false);
  const [unreadTick, setUnreadTick] = useState(0);
  const visibleCount = readVisibleCount();

  const load = useCallback(async () => {
    try {
      const list = await invoke<RecentSessionDto[]>("list_sessions", {
        filter: listKind,
        limit: projectId ? 50 : 200,
        projectId,
      });
      setItems(
        (list ?? []).map((item) => ({
          ...item,
          summary: visibleSessionTitle(item.summary, item.sessionId),
        })),
      );
    } catch {
      setItems([]);
    }
  }, [listKind, projectId]);

  useEffect(() => { void load(); }, [load]);

  useEffect(() => subscribeSessionsChanged(() => { void load(); }), [load]);

  useEffect(() => subscribeSessionUnread(() => setUnreadTick((n) => n + 1)), []);

  useEffect(() => {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    type SessionEventPayload = {
      sessionId?: string | null;
      sessionMetadataChanged?: { title: string } | null;
    };
    void listen<SessionEventPayload>("session_event", (ev) => {
      const sessionId = ev.payload.sessionId?.trim();
      const title = ev.payload.sessionMetadataChanged?.title?.trim();
      if (!sessionId || !title) return;
      setItems((prev) =>
        prev.map((item) =>
          item.sessionId === sessionId
            ? { ...item, summary: visibleSessionTitle(title, sessionId) }
            : item,
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

  const [sessionMenu, setSessionMenu] = useState<{ sessionId: string; x: number; y: number } | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!sessionMenu) return;
    const close = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) setSessionMenu(null);
    };
    const esc = (e: KeyboardEvent) => { if (e.key === "Escape") setSessionMenu(null); };
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", esc);
    return () => { window.removeEventListener("mousedown", close); window.removeEventListener("keydown", esc); };
  }, [sessionMenu]);

  // --- 会话操作 handlers ---

  const runSessionAction = useCallback(
    async (action: () => Promise<void>) => {
      setSessionMenu(null);
      try {
        await action();
        dispatchSessionsChanged();
      } catch (e) {
        showToast(
          t("sessions.actionFailed", {
            error: e instanceof Error ? e.message : String(e),
          }),
          { error: true },
        );
      }
    },
    [showToast, t],
  );

  const findMenuSession = useCallback((): RecentSessionDto | undefined => {
    if (!sessionMenu) return undefined;
    return items.find((s) => s.sessionId === sessionMenu.sessionId);
  }, [sessionMenu, items]);

  const handlePinToggle = useCallback((session: RecentSessionDto) => {
    const command = session.pinnedAt ? "unpin_session" : "pin_session";
    void runSessionAction(async () => {
      await invoke(command, { sessionId: session.sessionId });
    });
  }, [runSessionAction]);

  const handleArchiveToggle = useCallback((session: RecentSessionDto) => {
    const command = listKind === "archived" ? "unarchive_session" : "archive_session";
    void runSessionAction(async () => {
      await invoke(command, { sessionId: session.sessionId });
    });
  }, [listKind, runSessionAction]);

  const handleRename = useCallback(async () => {
    const session = findMenuSession();
    if (!session) return;
    setSessionMenu(null);
    const current = session.summary || t("chat.rightPanel.untitledSession");
    const next = await prompt({
      title: t("sessions.rename"),
      message: t("sessions.renamePrompt"),
      defaultValue: current,
      confirmLabel: t("sessions.renameSave"),
      cancelLabel: t("sessions.cancel"),
    });
    if (next === null) return;
    const title = next.trim();
    if (!title) return;
    await runSessionAction(async () => {
      await invoke("rename_session", { sessionId: session.sessionId, title });
      setItems((prev) =>
        prev.map((row) =>
          row.sessionId === session.sessionId ? { ...row, summary: title } : row,
        ),
      );
    });
  }, [findMenuSession, prompt, runSessionAction, t]);

  const handleRegenerateTitle = useCallback(() => {
    const session = findMenuSession();
    if (!session) return;
    void runSessionAction(async () => {
      const title = await invoke<string>("regenerate_session_title", {
        sessionId: session.sessionId,
      });
      const next = title.trim();
      if (!next) return;
      setItems((prev) =>
        prev.map((row) =>
          row.sessionId === session.sessionId ? { ...row, summary: next } : row,
        ),
      );
    });
  }, [findMenuSession, runSessionAction]);

  const handleExport = useCallback(() => {
    const session = findMenuSession();
    if (!session) return;
    void runSessionAction(async () => {
      const history = await invoke<ChatHistoryExportDto>("get_chat_history", {
        sessionId: session.sessionId,
        limit: 500,
      });
      const title = session.summary || t("chat.rightPanel.untitledSession");
      const markdown = historyToMarkdown(title, session.sessionId, history.messages ?? []);
      if (!markdown.replace(/^#.*$/m, "").trim()) {
        throw new Error(t("sessions.exportEmpty"));
      }
      const savedPath = await invoke<string>("download_bytes_to_downloads", {
        filename: sanitizeExportFilename(title, session.sessionId),
        base64Data: utf8ToBase64(markdown),
      });
      showToast(t("sessions.exportDone", { path: savedPath }), { tone: "success" });
    });
  }, [findMenuSession, runSessionAction, showToast, t]);

  const handleBranch = useCallback(() => {
    const session = findMenuSession();
    if (!session) return;
    void runSessionAction(async () => {
      const history = await invoke<ChatHistoryExportDto>("get_chat_history", {
        sessionId: session.sessionId,
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
        sourceSessionId: session.sessionId,
        keepChatBubbles: bubbles.length,
      });
      onOpenSession(newId);
    });
  }, [findMenuSession, onOpenSession, runSessionAction, t]);

  const handleDelete = useCallback(async () => {
    const session = findMenuSession();
    if (!session) return;
    setSessionMenu(null);
    const title = session.summary || t("chat.rightPanel.untitledSession");
    const ok = await confirm({
      title: t("sessions.deleteTitle"),
      emphasisLabel: t("sessions.deleteTargetLabel"),
      emphasis: title,
      message: t("sessions.deleteConfirm"),
      confirmLabel: t("sessions.deletePermanently"),
      cancelLabel: t("sessions.cancel"),
      variant: "danger",
    });
    if (!ok) return;
    await runSessionAction(async () => {
      if (session.sessionId === activeSessionId) {
        await onPrepareDeleteCurrentSession?.();
      }
      await deleteManagedSession(
        session.sessionId,
        activeSessionId,
        async () => {
          await invoke("delete_session_permanently", { sessionId: session.sessionId });
        },
        async () => {
          await onClearDeletedCurrentSession?.();
        },
      );
    });
  }, [
    findMenuSession,
    confirm,
    activeSessionId,
    onPrepareDeleteCurrentSession,
    onClearDeletedCurrentSession,
    runSessionAction,
    t,
  ]);

  const filtered = useMemo(() => {
    let result = items;
    if (pinnedFilter === "pinned") result = result.filter((s) => s.pinnedAt);
    else if (pinnedFilter === "unpinned") result = result.filter((s) => !s.pinnedAt);
    const q = query.trim().toLowerCase();
    if (!q) return result;
    return result.filter(
      (s) =>
        (s.summary ?? "").toLowerCase().includes(q) ||
        s.sessionId.toLowerCase().includes(q),
    );
  }, [items, query, pinnedFilter]);

  useEffect(() => { onCountChange?.(filtered.length); }, [filtered.length, onCountChange]);

  const hasQuery = query.trim().length > 0;
  const archived = listKind === "archived";
  const emptyLabel = hasQuery
    ? t("sessions.searchEmpty")
    : archived
      ? t("sessions.noArchived")
      : t("chat.rightPanel.noSessions");

  // 搜索结果不折叠，避免匹配项被藏在「展开显示」后面。
  const collapsible = !hasQuery && filtered.length > visibleCount;
  const visible = expanded || !collapsible ? filtered : filtered.slice(0, visibleCount);
  const hiddenCount = collapsible ? filtered.length - visibleCount : 0;

  return (
    <div className={`sidebar-sessions ${projectId ? "" : "is-global"}`.trim()}>
      {filtered.length === 0 ? (
        <span className="sidebar-empty">{emptyLabel}</span>
      ) : (
        visible.map((s) => (
          <SessionItem
            key={s.sessionId}
            session={s}
            t={t}
            isActive={s.sessionId === activeSessionId}
            archived={archived}
            status={resolveSessionStatus(sessionStatuses[s.sessionId])}
            unread={isSessionUnread(s.sessionId)}
            unreadTick={unreadTick}
            onOpen={() => {
              clearSessionUnread(s.sessionId);
              onOpenSession(s.sessionId);
            }}
            onContextMenu={(x, y) => setSessionMenu({ sessionId: s.sessionId, x, y })}
            onMoreClick={(x, y) => setSessionMenu({ sessionId: s.sessionId, x, y })}
            onPinToggle={() => handlePinToggle(s)}
            onArchiveToggle={() => handleArchiveToggle(s)}
          />
        ))
      )}
      {hiddenCount > 0 && (
        <button
          type="button"
          className="sidebar-session-expand"
          onClick={() => setExpanded((v) => !v)}
        >
          <MorphToggleIcon
            active={expanded}
            activeIcon={ChevronUpData}
            inactiveIcon={ChevronDownData}
            size={12}
            strokeWidth={2}
            aria-hidden
          />
          <span>
            {expanded
              ? t("sessions.collapse")
              : t("sessions.expandMore", {
                  n: hiddenCount > 99 ? "99+" : String(hiddenCount),
                })}
          </span>
        </button>
      )}
      {sessionMenu && (() => {
        const menuSession = items.find((s) => s.sessionId === sessionMenu.sessionId);
        const pinned = Boolean(menuSession?.pinnedAt);
        return createPortal(
          <div ref={menuRef} className="project-context-menu" style={{ top: sessionMenu.y, left: sessionMenu.x }} role="menu">
            <button type="button" role="menuitem" className="project-context-menu-item" onClick={() => { if (menuSession) handlePinToggle(menuSession); }}>
              <MorphToggleIcon
                active={pinned}
                activeIcon={PinOffData}
                inactiveIcon={PinData}
                size={14}
                strokeWidth={1.8}
                aria-hidden
              />
              <span>{pinned ? t("sessions.unpin") : t("sessions.pin")}</span>
            </button>
            <button type="button" role="menuitem" className="project-context-menu-item" onClick={() => void handleRename()}>
              <Edit3 size={14} strokeWidth={1.8} aria-hidden /><span>{t("sessions.rename")}</span>
            </button>
            <button type="button" role="menuitem" className="project-context-menu-item" onClick={() => handleRegenerateTitle()}>
              <RefreshCw size={14} strokeWidth={1.8} aria-hidden /><span>{t("sessions.regenerateTitle")}</span>
            </button>
            <button type="button" role="menuitem" className="project-context-menu-item" onClick={() => handleExport()}>
              <Download size={14} strokeWidth={1.8} aria-hidden /><span>{t("sessions.export")}</span>
            </button>
            <button type="button" role="menuitem" className="project-context-menu-item" onClick={() => handleBranch()}>
              <GitBranch size={14} strokeWidth={1.8} aria-hidden /><span>{t("sessions.branch")}</span>
            </button>
            <button type="button" role="menuitem" className="project-context-menu-item" onClick={() => { if (menuSession) handleArchiveToggle(menuSession); }}>
              <MorphToggleIcon
                active={archived}
                activeIcon={ArchiveRestoreData}
                inactiveIcon={ArchiveData}
                size={14}
                strokeWidth={1.8}
                aria-hidden
              />
              <span>{archived ? t("sessions.unarchive") : t("sessions.archive")}</span>
            </button>
            <button type="button" role="menuitem" className="project-context-menu-item is-danger" onClick={() => void handleDelete()}>
              <Trash2 size={14} strokeWidth={1.8} aria-hidden /><span>{t("sessions.deletePermanently")}</span>
            </button>
          </div>,
          document.body,
        );
      })()}
      {createPortal(toastHost, document.body)}
    </div>
  );
}

function SessionItem({
  session: s,
  t,
  isActive,
  archived,
  status,
  unread,
  unreadTick,
  onOpen,
  onContextMenu,
  onMoreClick,
  onPinToggle,
  onArchiveToggle,
}: {
  session: RecentSessionDto;
  t: Translate;
  isActive: boolean;
  archived: boolean;
  status: SessionActivityStatus;
  unread: boolean;
  /** 未读集合变更计数；仅用于触发重渲染 */
  unreadTick: number;
  onOpen: () => void;
  onContextMenu: (x: number, y: number) => void;
  onMoreClick: (x: number, y: number) => void;
  onPinToggle: () => void;
  onArchiveToggle: () => void;
}) {
  const titleRef = useRef<HTMLSpanElement>(null);
  void unreadTick;

  const handleMouseEnter = () => {
    const el = titleRef.current;
    const wrap = el?.parentElement;
    if (!el || !wrap) return;
    const overflow = el.scrollWidth - wrap.clientWidth;
    if (overflow > 1) {
      el.dataset.scrollable = "true";
      el.style.setProperty("--scroll-distance", `-${overflow + 4}px`);
    } else {
      delete el.dataset.scrollable;
      el.style.removeProperty("--scroll-distance");
    }
  };

  const showUnread = unread && status === "idle";

  return (
    <div
      className={`sidebar-session-item ${isActive ? "is-active" : ""} ${
        status === "awaiting" ? "is-awaiting" : ""
      } ${status === "error" ? "is-errored" : ""} ${showUnread ? "is-unread" : ""}`}
      onContextMenu={(e) => { e.preventDefault(); e.stopPropagation(); onContextMenu(e.clientX, e.clientY); }}
      onMouseEnter={handleMouseEnter}
    >
      <button type="button" className="sidebar-session-main" onClick={onOpen}>
        <span className="sidebar-session-title-wrap">
          <span className="sidebar-session-title" ref={titleRef}>
            {s.summary || t("chat.rightPanel.untitledSession")}
          </span>
        </span>
        {s.pinnedAt && (
          <Pin className="sidebar-session-pin-mark" size={10} strokeWidth={2} aria-hidden />
        )}
        {status === "idle" && (
          <span className="sidebar-session-time">{relativeTime(s.createdAt, t)}</span>
        )}
        <SessionStatusIcon
          status={status}
          unread={showUnread}
          label={t(
            showUnread
              ? "sessions.status.unread"
              : (`sessions.status.${status}` as MessageKey),
          )}
        />
      </button>
      <div className="sidebar-session-actions">
        <button type="button" className="sidebar-session-action-btn" title={s.pinnedAt ? t("sessions.unpin") : t("sessions.pin")} onClick={(e) => { e.stopPropagation(); onPinToggle(); }}>
          <MorphToggleIcon
            active={Boolean(s.pinnedAt)}
            activeIcon={PinOffData}
            inactiveIcon={PinData}
            size={13}
            strokeWidth={1.8}
            aria-hidden
          />
        </button>
        <button type="button" className="sidebar-session-action-btn" title={archived ? t("sessions.unarchive") : t("sessions.archive")} onClick={(e) => { e.stopPropagation(); onArchiveToggle(); }}>
          <MorphToggleIcon
            active={archived}
            activeIcon={ArchiveRestoreData}
            inactiveIcon={ArchiveData}
            size={13}
            strokeWidth={1.8}
            aria-hidden
          />
        </button>
        <button
          type="button"
          className="sidebar-session-action-btn"
          title={t("sessions.moreActions")}
          onClick={(e) => {
            e.stopPropagation();
            const rect = e.currentTarget.getBoundingClientRect();
            onMoreClick(rect.right + 4, rect.top);
          }}
        >
          <MoreVertical size={13} strokeWidth={1.8} aria-hidden />
        </button>
      </div>
    </div>
  );
}
