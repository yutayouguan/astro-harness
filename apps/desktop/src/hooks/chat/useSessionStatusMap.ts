import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { markSessionUnread } from "../../lib/chat/sessionUnread";

export type SessionRuntimeActiveFlag =
  | "waitingOnApproval"
  | "waitingOnUserInput";

export type SessionRuntimeStatus = {
  status: "idle" | "active" | "systemError";
  activeFlags: SessionRuntimeActiveFlag[];
  error: string | null;
  updatedAt: number;
};

export type SessionStatusMap = Readonly<Record<string, SessionRuntimeStatus>>;

type SessionStatusChangedPayload = {
  sessionId?: unknown;
  status?: unknown;
  activeFlags?: unknown;
  error?: unknown;
  tsMs?: unknown;
};

function normalizeStatus(
  payload: SessionStatusChangedPayload,
): [string, SessionRuntimeStatus] | null {
  const sessionId =
    typeof payload.sessionId === "string" ? payload.sessionId.trim() : "";
  if (!sessionId) return null;
  if (
    payload.status !== "idle" &&
    payload.status !== "active" &&
    payload.status !== "systemError"
  ) {
    return null;
  }
  const activeFlags = Array.isArray(payload.activeFlags)
    ? payload.activeFlags.filter(
        (flag): flag is SessionRuntimeActiveFlag =>
          flag === "waitingOnApproval" || flag === "waitingOnUserInput",
      )
    : [];
  return [
    sessionId,
    {
      status: payload.status,
      activeFlags,
      error: typeof payload.error === "string" ? payload.error : null,
      updatedAt: typeof payload.tsMs === "number" ? payload.tsMs : Date.now(),
    },
  ];
}

/**
 * 进程级 Thread 桥会发布所有已订阅会话的状态；此 hook 将其投影为
 * sessionId 索引，供左右会话列表共同消费。
 */
export function useSessionStatusMap(): SessionStatusMap {
  const [statuses, setStatuses] = useState<
    Record<string, SessionRuntimeStatus>
  >({});
  const statusesRef = useRef<Record<string, SessionRuntimeStatus>>({});

  useEffect(() => {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
      return;
    }
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const applyStatus = (payload: SessionStatusChangedPayload) => {
      const normalized = normalizeStatus(payload);
      if (!normalized || disposed) return;
      const [sessionId, next] = normalized;
      const previous = statusesRef.current[sessionId];
      if (previous && previous.updatedAt >= next.updatedAt) return;
      // 离开 active 即视为有新结果待查看；点开会话时清除。
      if (previous?.status === "active" && next.status !== "active") {
        markSessionUnread(sessionId);
      }
      statusesRef.current = { ...statusesRef.current, [sessionId]: next };
      setStatuses(statusesRef.current);
    };
    const setup = async () => {
      try {
        const stop = await listen<SessionStatusChangedPayload>(
          "session_status_changed",
          ({ payload }) => applyStatus(payload),
        );
        if (disposed) {
          stop();
          return;
        }
        unlisten = stop;
        // 先订阅再取快照；若两者交错，updatedAt 会拒绝较旧的快照。
        void invoke<SessionStatusChangedPayload[]>("list_session_statuses")
          .then((snapshot) => snapshot.forEach(applyStatus))
          .catch(() => {});
      } catch {
        // Web preview or shell teardown can make the Tauri event API unavailable.
      }
    };
    void setup();
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  return statuses;
}
