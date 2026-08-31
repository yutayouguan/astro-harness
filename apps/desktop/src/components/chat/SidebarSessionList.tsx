// 侧栏会话列表：按项目分组或全局搜索结果，含全部会话操作。

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { MoreVertical, Pin } from "lucide-react";
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
  dispatchSessionsChanged,
  subscribeSessionsChanged,
  type SessionListKind,
} from "../../lib/chat/sessionManagement";
import {
  clearSessionUnread,
  isSessionUnread,
  subscribeSessionUnread,
} from "../../lib/chat/sessionUnread";
import { useI18n } from "../../i18n/LocaleContext";
import { useTransientToast } from "../../hooks/ui/useTransientToast";
import type { MessageKey } from "../../i18n/messages";
import type { RecentSessionDto } from "../../types";
import type { SessionStatusMap } from "../../hooks/chat/useSessionStatusMap";
import { visibleSessionTitle } from "../../lib/chat/sessionTitle";
import {
  matchesSidebarSessionPlacement,
  type SidebarSessionPlacement,
} from "../../lib/chat/sidebarSessionPlacement";
import SessionActionsMenu from "./SessionActionsMenu";
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
    if (v) {
      const n = Number(v);
      if (n >= 1 && n <= 50) return n;
    }
  } catch {}
  return DEFAULT_VISIBLE_COUNT;
}

type Props = {
  activeSessionId: string | null;
  sessionStatuses: SessionStatusMap;
  /** null 表示跨项目的全局列表：不按项目过滤，也不自动关联会话 */
  projectId: string | null;
  /** 侧栏全局搜索词；非空时展示全部匹配结果 */
  query: string;
  listKind: SessionListKind;
  /** 互斥的侧栏展示位置；不会修改会话项目归属。 */
  placement?: SidebarSessionPlacement;
  /** 过滤后条目数变化时回调，用于外部按需隐藏整个分区 */
  onCountChange?: (count: number) => void;
  onOpenSession: (sessionId: string) => void;
  /** 删除当前会话前取消流 */
  onPrepareDeleteCurrentSession?: () => void | Promise<void>;
  /** 当前会话被删除或移出当前项目后清理本地状态 */
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
  placement = "all",
  onCountChange,
  onOpenSession,
  onPrepareDeleteCurrentSession,
  onClearDeletedCurrentSession,
}: Props) {
  const { t } = useI18n();
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
        placement,
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
  }, [listKind, placement, projectId]);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(
    () =>
      subscribeSessionsChanged(() => {
        void load();
      }),
    [load],
  );

  useEffect(
    () => subscribeSessionUnread(() => setUnreadTick((n) => n + 1)),
    [],
  );

  useEffect(() => {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window))
      return;
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

  const [sessionMenu, setSessionMenu] = useState<{
    sessionId: string;
    x: number;
    y: number;
  } | null>(null);

  const openSessionMenu = useCallback(
    (sessionId: string, x: number, y: number) => {
      setSessionMenu({ sessionId, x, y });
    },
    [],
  );

  const runQuickAction = useCallback(
    async (command: string, sessionId: string) => {
      try {
        await invoke(command, { sessionId });
        dispatchSessionsChanged();
      } catch (error) {
        showToast(
          t("sessions.actionFailed", {
            error: error instanceof Error ? error.message : String(error),
          }),
          { error: true },
        );
      }
    },
    [showToast, t],
  );

  const filtered = useMemo(() => {
    let result = items.filter((session) =>
      matchesSidebarSessionPlacement(session, placement),
    );
    const q = query.trim().toLowerCase();
    if (!q) return result;
    return result.filter(
      (s) =>
        (s.summary ?? "").toLowerCase().includes(q) ||
        s.sessionId.toLowerCase().includes(q),
    );
  }, [items, placement, query]);

  useEffect(() => {
    onCountChange?.(filtered.length);
  }, [filtered.length, onCountChange]);

  const hasQuery = query.trim().length > 0;
  const archived = listKind === "archived";
  const emptyLabel = hasQuery
    ? t("sessions.searchEmpty")
    : archived
      ? t("sessions.noArchived")
      : t("chat.rightPanel.noSessions");

  // 搜索结果不折叠，避免匹配项被藏在「展开显示」后面。
  const collapsible = !hasQuery && filtered.length > visibleCount;
  const visible =
    expanded || !collapsible ? filtered : filtered.slice(0, visibleCount);
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
            onContextMenu={(x, y) => openSessionMenu(s.sessionId, x, y)}
            onMoreClick={(x, y) => openSessionMenu(s.sessionId, x, y)}
            onPinToggle={() =>
              void runQuickAction(
                s.pinnedAt ? "unpin_session" : "pin_session",
                s.sessionId,
              )
            }
            onArchiveToggle={() =>
              void runQuickAction(
                archived ? "unarchive_session" : "archive_session",
                s.sessionId,
              )
            }
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
      {sessionMenu &&
        (() => {
          const menuSession = items.find(
            (item) => item.sessionId === sessionMenu.sessionId,
          );
          if (!menuSession) return null;
          return (
            <SessionActionsMenu
              session={{
                ...menuSession,
                projectId: menuSession.projectId ?? projectId,
              }}
              x={sessionMenu.x}
              y={sessionMenu.y}
              status={resolveSessionStatus(
                sessionStatuses[sessionMenu.sessionId],
              )}
              activeSessionId={activeSessionId}
              onClose={() => setSessionMenu(null)}
              onOpenSession={onOpenSession}
              showToast={showToast}
              onPrepareDeleteCurrentSession={onPrepareDeleteCurrentSession}
              onClearDeletedCurrentSession={onClearDeletedCurrentSession}
            />
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
      onContextMenu={(e) => {
        e.preventDefault();
        e.stopPropagation();
        onContextMenu(e.clientX, e.clientY);
      }}
      onMouseEnter={handleMouseEnter}
    >
      <button
        type="button"
        className="sidebar-session-main"
        aria-current={isActive ? "page" : undefined}
        onClick={onOpen}
      >
        <span className="sidebar-session-title-wrap">
          <span className="sidebar-session-title" ref={titleRef}>
            {s.summary || t("chat.rightPanel.untitledSession")}
          </span>
        </span>
        {s.pinnedAt && (
          <Pin
            className="sidebar-session-pin-mark"
            size={10}
            strokeWidth={2}
            aria-hidden
          />
        )}
        {status === "idle" && (
          <span className="sidebar-session-time">
            {relativeTime(s.createdAt, t)}
          </span>
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
        <button
          type="button"
          className="sidebar-session-action-btn"
          title={s.pinnedAt ? t("sessions.unpin") : t("sessions.pin")}
          onClick={(e) => {
            e.stopPropagation();
            onPinToggle();
          }}
        >
          <MorphToggleIcon
            active={Boolean(s.pinnedAt)}
            activeIcon={PinOffData}
            inactiveIcon={PinData}
            size={13}
            strokeWidth={1.8}
            aria-hidden
          />
        </button>
        <button
          type="button"
          className="sidebar-session-action-btn"
          title={archived ? t("sessions.unarchive") : t("sessions.archive")}
          onClick={(e) => {
            e.stopPropagation();
            onArchiveToggle();
          }}
        >
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
