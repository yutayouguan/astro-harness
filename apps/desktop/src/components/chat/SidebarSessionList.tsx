// 侧栏项目下的会话列表（轻量版，直接调 list_sessions）。

import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { MessageSquare } from "lucide-react";
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
        <button
          key={s.sessionId}
          type="button"
          className={`sidebar-session-item ${s.sessionId === activeSessionId ? "is-active" : ""}`}
          onClick={() => onOpenSession(s.sessionId)}
          title={s.summary || "未命名会话"}
        >
          <MessageSquare size={13} strokeWidth={1.6} aria-hidden />
          <span className="sidebar-session-title">
            {s.summary || "未命名会话"}
          </span>
          <span className="sidebar-session-time">
            {relativeTime(s.createdAt)}
          </span>
        </button>
      ))}
    </div>
  );
}
