// 侧栏项目下的会话列表（轻量版，直接调 list_sessions）。

import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Archive, MessageSquare, Pin, Trash2 } from "lucide-react";
import type { RecentSessionDto } from "../../types";

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

  const load = useCallback(async () => {
    try {
      const list = await invoke<RecentSessionDto[]>("list_sessions", {
        filter: "active",
        limit: 20,
      });
      setItems(list ?? []);
    } catch {
      setItems([]);
    }
  }, []);

  useEffect(() => { void load(); }, [load]);

  // 每次 activeSessionId 变化时刷新（新建会话后）
  useEffect(() => { void load(); }, [activeSessionId, load]);

  if (items.length === 0) {
    return (
      <div className="sidebar-sessions">
        <span className="sidebar-empty">暂无会话</span>
      </div>
    );
  }

  return (
    <div className="sidebar-sessions">
      {items.map((s) => (
        <SessionItem
          key={s.sessionId}
          session={s}
          isActive={s.sessionId === activeSessionId}
          onOpen={() => onOpenSession(s.sessionId)}
        />
      ))}
    </div>
  );
}

type SessionMenuAction = "pin" | "archive" | "delete";

function SessionItem({
  session: s,
  isActive,
  onOpen,
}: {
  session: RecentSessionDto;
  isActive: boolean;
  onOpen: () => void;
}) {
  const titleRef = useRef<HTMLSpanElement>(null);
  const [menuPos, setMenuPos] = useState<{ x: number; y: number } | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  const handleMouseEnter = () => {
    const el = titleRef.current;
    if (!el) return;
    const overflow = el.scrollWidth - el.clientWidth;
    if (overflow > 0) {
      el.style.setProperty("--scroll-distance", `-${overflow}px`);
    }
  };

  useEffect(() => {
    if (!menuPos) return;
    const close = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) setMenuPos(null);
    };
    const esc = (e: KeyboardEvent) => { if (e.key === "Escape") setMenuPos(null); };
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", esc);
    return () => { window.removeEventListener("mousedown", close); window.removeEventListener("keydown", esc); };
  }, [menuPos]);

  const handleAction = (action: SessionMenuAction) => {
    void action;
    setMenuPos(null);
  };

  return (
    <>
      <button
        type="button"
        className={`sidebar-session-item ${isActive ? "is-active" : ""}`}
        onClick={onOpen}
        onMouseEnter={handleMouseEnter}
        onContextMenu={(e) => { e.preventDefault(); setMenuPos({ x: e.clientX, y: e.clientY }); }}
        title={s.summary || "未命名会话"}
      >
        <MessageSquare size={13} strokeWidth={1.6} aria-hidden />
        <span className="sidebar-session-title" ref={titleRef}>
          {s.summary || "未命名会话"}
        </span>
        <span className="sidebar-session-time">
          {relativeTime(s.createdAt)}
        </span>
      </button>
      {menuPos && (
        <div ref={menuRef} className="project-context-menu" style={{ top: menuPos.y, left: menuPos.x }} role="menu">
          <button type="button" role="menuitem" className="project-context-menu-item" onClick={() => handleAction("pin")}>
            <Pin size={14} strokeWidth={1.8} aria-hidden /><span>置顶</span>
          </button>
          <button type="button" role="menuitem" className="project-context-menu-item" onClick={() => handleAction("archive")}>
            <Archive size={14} strokeWidth={1.8} aria-hidden /><span>归档</span>
          </button>
          <button type="button" role="menuitem" className="project-context-menu-item is-danger" onClick={() => handleAction("delete")}>
            <Trash2 size={14} strokeWidth={1.8} aria-hidden /><span>删除</span>
          </button>
        </div>
      )}
    </>
  );
}
