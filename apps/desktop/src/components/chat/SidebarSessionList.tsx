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

type Props = {
  activeSessionId: string | null;
  onOpenSession: (sessionId: string) => void;
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

export default function SidebarSessionList({ activeSessionId, onOpenSession }: Props) {
  const [items, setItems] = useState<RecentSessionDto[]>([]);
  const [expanded, setExpanded] = useState(false);
  const visibleCount = readVisibleCount();

  const load = useCallback(async () => {
    try {
      const list = await invoke<RecentSessionDto[]>("list_sessions", {
        filter: "active",
        limit: 50,
      });
      setItems(list ?? []);
    } catch {
      setItems([]);
    }
  }, []);

  useEffect(() => { void load(); }, [load]);
  useEffect(() => { void load(); }, [activeSessionId, load]);

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
      {sessionMenu && createPortal(
        <div ref={menuRef} className="project-context-menu" style={{ top: sessionMenu.y, left: sessionMenu.x }} role="menu">
          <button type="button" role="menuitem" className="project-context-menu-item" onClick={() => setSessionMenu(null)}>
            <Pin size={14} strokeWidth={1.8} aria-hidden /><span>置顶</span>
          </button>
          <button type="button" role="menuitem" className="project-context-menu-item" onClick={() => setSessionMenu(null)}>
            <Edit3 size={14} strokeWidth={1.8} aria-hidden /><span>重命名</span>
          </button>
          <button type="button" role="menuitem" className="project-context-menu-item" onClick={() => setSessionMenu(null)}>
            <RefreshCw size={14} strokeWidth={1.8} aria-hidden /><span>重新生成标题</span>
          </button>
          <button type="button" role="menuitem" className="project-context-menu-item" onClick={() => setSessionMenu(null)}>
            <Download size={14} strokeWidth={1.8} aria-hidden /><span>导出</span>
          </button>
          <button type="button" role="menuitem" className="project-context-menu-item" onClick={() => setSessionMenu(null)}>
            <GitBranch size={14} strokeWidth={1.8} aria-hidden /><span>分支</span>
          </button>
          <button type="button" role="menuitem" className="project-context-menu-item" onClick={() => setSessionMenu(null)}>
            <Archive size={14} strokeWidth={1.8} aria-hidden /><span>归档</span>
          </button>
          <button type="button" role="menuitem" className="project-context-menu-item is-danger" onClick={() => setSessionMenu(null)}>
            <Trash2 size={14} strokeWidth={1.8} aria-hidden /><span>永久删除</span>
          </button>
        </div>,
        document.body,
      )}
    </div>
  );
}

function SessionItem({
  session: s,
  isActive,
  onOpen,
  onContextMenu,
  onMoreClick,
}: {
  session: RecentSessionDto;
  isActive: boolean;
  onOpen: () => void;
  onContextMenu: (x: number, y: number) => void;
  onMoreClick: (x: number, y: number) => void;
}) {
  const titleRef = useRef<HTMLSpanElement>(null);

  const handleMouseEnter = () => {
    const el = titleRef.current;
    if (!el) return;
    const overflow = el.scrollWidth - el.clientWidth;
    if (overflow > 0) {
      el.style.setProperty("--scroll-distance", `-${overflow}px`);
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
        <span className="sidebar-session-title" ref={titleRef}>
          {s.summary || "未命名会话"}
        </span>
        <span className="sidebar-session-time">
          {relativeTime(s.createdAt)}
        </span>
      </button>
      <div className="sidebar-session-actions">
        <button type="button" className="sidebar-session-action-btn" title="置顶" onClick={(e) => { e.stopPropagation(); }}>
          <Pin size={13} strokeWidth={1.8} aria-hidden />
        </button>
        <button type="button" className="sidebar-session-action-btn" title="归档" onClick={(e) => { e.stopPropagation(); }}>
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
