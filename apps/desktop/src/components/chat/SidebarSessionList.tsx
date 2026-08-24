// 侧栏项目下的会话列表。

import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import {
  Archive,
  ChevronDown,
  Download,
  Edit3,
  GitBranch,
  MessageSquare,
  MoreVertical,
  Pin,
  PinOff,
  RefreshCw,
  Trash2,
} from "lucide-react";
import type { RecentSessionDto } from "../../types";

const DEFAULT_VISIBLE_COUNT = 5;
const STORAGE_KEY = "astro:sidebar-visible-sessions";
function readVisibleCount(): number {
  try {
    const v = localStorage.getItem(STORAGE_KEY);
    if (v) { const n = Number(v); if (n >= 1 && n <= 50) return n; }
  } catch {}
  return DEFAULT_VISIBLE_COUNT;
}

export function saveVisibleCount(count: number) {
  try { localStorage.setItem(STORAGE_KEY, String(Math.max(1, Math.min(50, count)))); } catch {}
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
  projectId: string;
  onOpenSession: (sessionId: string) => void;
  onDeleteCurrentSession?: () => void;
};

function relativeTime(iso: string | null): string {
  if (!iso) return "";
  const diff = Date.now() - new Date(iso).getTime();
  const mins = Math.floor(diff / 60000);
  if (mins < 1) return "刚刚";
  if (mins < 60) return `${mins} 分钟前`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours} 小时前`;
  const days = Math.floor(hours / 24);
  return `${days} 天前`;
}

export default function SidebarSessionList({ activeSessionId, projectId, onOpenSession, onDeleteCurrentSession }: Props) {
  const [items, setItems] = useState<RecentSessionDto[]>([]);
  const [expanded, setExpanded] = useState(false);
  const visibleCount = readVisibleCount();

  const load = useCallback(async () => {
    try {
      const list = await invoke<RecentSessionDto[]>("list_sessions", {
        filter: "active",
        limit: 50,
        projectId,
      });
      setItems(list ?? []);
    } catch {
      setItems([]);
    }
  }, [projectId]);

  useEffect(() => { void load(); }, [load]);

  // 当前会话自动关联到此项目，关联完成后再刷新列表
  useEffect(() => {
    if (!activeSessionId) return;
    void (async () => {
      try {
        await invoke("assign_session_to_project", {
          sessionId: activeSessionId,
          projectId,
        });
      } catch {}
      await load();
    })();
  }, [activeSessionId, projectId, load]);

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

  const findMenuSession = useCallback((): RecentSessionDto | undefined => {
    if (!sessionMenu) return undefined;
    return items.find((s) => s.sessionId === sessionMenu.sessionId);
  }, [sessionMenu, items]);

  const handlePinToggle = useCallback(async (session: RecentSessionDto) => {
    setSessionMenu(null);
    const command = session.pinnedAt ? "unpin_session" : "pin_session";
    try {
      await invoke(command, { sessionId: session.sessionId });
      await load();
    } catch (e) { console.warn("pin/unpin failed", e); }
  }, [load]);

  const handleArchive = useCallback(async (session: RecentSessionDto) => {
    setSessionMenu(null);
    try {
      await invoke("archive_session", { sessionId: session.sessionId });
      await load();
    } catch (e) { console.warn("archive failed", e); }
  }, [load]);

  const handleRename = useCallback(async () => {
    const session = findMenuSession();
    if (!session) return;
    setSessionMenu(null);
    const current = session.summary || "未命名会话";
    const next = window.prompt("输入新标题", current);
    if (next === null) return;
    const title = next.trim();
    if (!title) return;
    try {
      await invoke("rename_session", { sessionId: session.sessionId, title });
      setItems((prev) =>
        prev.map((row) =>
          row.sessionId === session.sessionId ? { ...row, summary: title } : row,
        ),
      );
    } catch (e) { console.warn("rename failed", e); }
  }, [findMenuSession]);

  const handleRegenerateTitle = useCallback(async () => {
    const session = findMenuSession();
    if (!session) return;
    setSessionMenu(null);
    try {
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
    } catch (e) { console.warn("regenerate title failed", e); }
  }, [findMenuSession]);

  const handleExport = useCallback(async () => {
    const session = findMenuSession();
    if (!session) return;
    setSessionMenu(null);
    try {
      const history = await invoke<ChatHistoryExportDto>("get_chat_history", {
        sessionId: session.sessionId,
        limit: 500,
      });
      const title = session.summary || "未命名会话";
      const markdown = historyToMarkdown(title, session.sessionId, history.messages ?? []);
      if (!markdown.replace(/^#.*$/m, "").trim()) return;
      await invoke<string>("download_bytes_to_downloads", {
        filename: sanitizeExportFilename(title, session.sessionId),
        base64Data: utf8ToBase64(markdown),
      });
    } catch (e) { console.warn("export failed", e); }
  }, [findMenuSession]);

  const handleBranch = useCallback(async () => {
    const session = findMenuSession();
    if (!session) return;
    setSessionMenu(null);
    try {
      const history = await invoke<ChatHistoryExportDto>("get_chat_history", {
        sessionId: session.sessionId,
        limit: 500,
      });
      const bubbles = (history.messages ?? []).filter((m) => {
        const role = m.role.trim().toLowerCase();
        return role === "user" || role === "assistant";
      });
      if (bubbles.length === 0) return;
      const newId = await invoke<string>("fork_chat_session", {
        sourceSessionId: session.sessionId,
        keepChatBubbles: bubbles.length,
      });
      onOpenSession(newId);
      await load();
    } catch (e) { console.warn("branch failed", e); }
  }, [findMenuSession, onOpenSession, load]);

  const handleDelete = useCallback(async () => {
    const session = findMenuSession();
    if (!session) return;
    setSessionMenu(null);
    const title = session.summary || "未命名会话";
    const ok = window.confirm(`确定永久删除会话「${title}」吗？此操作不可撤销。`);
    if (!ok) return;
    try {
      if (session.sessionId === activeSessionId) {
        onDeleteCurrentSession?.();
      }
      await invoke("delete_session_permanently", { sessionId: session.sessionId });
      await load();
    } catch (e) { console.warn("delete failed", e); }
  }, [findMenuSession, activeSessionId, onDeleteCurrentSession, load]);

  if (items.length === 0) {
    return (
      <div className="sidebar-sessions">
        <span className="sidebar-empty">暂无会话</span>
      </div>
    );
  }

  const visible = expanded ? items : items.slice(0, visibleCount);
  const hiddenCount = items.length - visibleCount;

  return (
    <div className="sidebar-sessions">
      {visible.map((s) => (
        <SessionItem
          key={s.sessionId}
          session={s}
          isActive={s.sessionId === activeSessionId}
          onOpen={() => onOpenSession(s.sessionId)}
          onContextMenu={(x, y) => setSessionMenu({ sessionId: s.sessionId, x, y })}
          onMoreClick={(x, y) => setSessionMenu({ sessionId: s.sessionId, x, y })}
          onPinToggle={() => void handlePinToggle(s)}
          onArchive={() => void handleArchive(s)}
        />
      ))}
      {hiddenCount > 0 && (
        <button
          type="button"
          className="sidebar-session-expand"
          onClick={() => setExpanded((v) => !v)}
        >
          <ChevronDown
            size={12} strokeWidth={2}
            className={expanded ? "is-open" : ""}
            aria-hidden
          />
          <span>
            {expanded
              ? "收起"
              : `展开显示 ${hiddenCount > 99 ? "99+" : hiddenCount} 条`}
          </span>
        </button>
      )}
      {sessionMenu && (() => {
        const menuSession = items.find((s) => s.sessionId === sessionMenu.sessionId);
        const pinned = Boolean(menuSession?.pinnedAt);
        return createPortal(
          <div ref={menuRef} className="project-context-menu" style={{ top: sessionMenu.y, left: sessionMenu.x }} role="menu">
            <button type="button" role="menuitem" className="project-context-menu-item" onClick={() => { if (menuSession) void handlePinToggle(menuSession); }}>
              {pinned ? <PinOff size={14} strokeWidth={1.8} aria-hidden /> : <Pin size={14} strokeWidth={1.8} aria-hidden />}
              <span>{pinned ? "取消置顶" : "置顶"}</span>
            </button>
            <button type="button" role="menuitem" className="project-context-menu-item" onClick={() => void handleRename()}>
              <Edit3 size={14} strokeWidth={1.8} aria-hidden /><span>重命名</span>
            </button>
            <button type="button" role="menuitem" className="project-context-menu-item" onClick={() => void handleRegenerateTitle()}>
              <RefreshCw size={14} strokeWidth={1.8} aria-hidden /><span>重新生成标题</span>
            </button>
            <button type="button" role="menuitem" className="project-context-menu-item" onClick={() => void handleExport()}>
              <Download size={14} strokeWidth={1.8} aria-hidden /><span>导出</span>
            </button>
            <button type="button" role="menuitem" className="project-context-menu-item" onClick={() => void handleBranch()}>
              <GitBranch size={14} strokeWidth={1.8} aria-hidden /><span>分支</span>
            </button>
            <button type="button" role="menuitem" className="project-context-menu-item" onClick={() => { if (menuSession) void handleArchive(menuSession); }}>
              <Archive size={14} strokeWidth={1.8} aria-hidden /><span>归档</span>
            </button>
            <button type="button" role="menuitem" className="project-context-menu-item is-danger" onClick={() => void handleDelete()}>
              <Trash2 size={14} strokeWidth={1.8} aria-hidden /><span>永久删除</span>
            </button>
          </div>,
          document.body,
        );
      })()}
    </div>
  );
}

function SessionItem({
  session: s,
  isActive,
  onOpen,
  onContextMenu,
  onMoreClick,
  onPinToggle,
  onArchive,
}: {
  session: RecentSessionDto;
  isActive: boolean;
  onOpen: () => void;
  onContextMenu: (x: number, y: number) => void;
  onMoreClick: (x: number, y: number) => void;
  onPinToggle: () => void;
  onArchive: () => void;
}) {
  const titleRef = useRef<HTMLSpanElement>(null);

  const handleMouseEnter = () => {
    const el = titleRef.current;
    const wrap = el?.parentElement;
    if (!el || !wrap) return;
    const overflow = el.scrollWidth - wrap.clientWidth;
    if (overflow > 0) {
      el.style.setProperty("--scroll-distance", `-${overflow + 4}px`);
    } else {
      el.style.removeProperty("--scroll-distance");
    }
  };

  return (
    <div
      className={`sidebar-session-item ${isActive ? "is-active" : ""}`}
      onContextMenu={(e) => { e.preventDefault(); e.stopPropagation(); onContextMenu(e.clientX, e.clientY); }}
      onMouseEnter={handleMouseEnter}
    >
      <button type="button" className="sidebar-session-main" onClick={onOpen}>
        <MessageSquare size={13} strokeWidth={1.6} aria-hidden />
        <span className="sidebar-session-title-wrap">
          <span className="sidebar-session-title" ref={titleRef}>
            {s.summary || "未命名会话"}
          </span>
        </span>
        <span className="sidebar-session-time">
          {relativeTime(s.createdAt)}
        </span>
      </button>
      <div className="sidebar-session-actions">
        <button type="button" className="sidebar-session-action-btn" title={s.pinnedAt ? "取消置顶" : "置顶"} onClick={(e) => { e.stopPropagation(); onPinToggle(); }}>
          {s.pinnedAt ? <PinOff size={13} strokeWidth={1.8} aria-hidden /> : <Pin size={13} strokeWidth={1.8} aria-hidden />}
        </button>
        <button type="button" className="sidebar-session-action-btn" title="归档" onClick={(e) => { e.stopPropagation(); onArchive(); }}>
          <Archive size={13} strokeWidth={1.8} aria-hidden />
        </button>
        <button
          type="button"
          className="sidebar-session-action-btn"
          title="更多"
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
