import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { subscribeSessionsChanged } from "../../lib/chat/sessionManagement";
import type { RecentSessionDto } from "../../types";

type SessionEventPayload = {
  sessionId?: string | null;
  sessionMetadataChanged?: { title?: string | null } | null;
};

/**
 * 当前会话的标题。标题由后端在首轮结束后生成，所以新会话在拿到
 * session_event 之前一直是 null——调用方据此决定是否渲染标题。
 */
export function useActiveSessionMetadata(
  sessionId: string | null,
): RecentSessionDto | null {
  const [session, setSession] = useState<RecentSessionDto | null>(null);

  const load = useCallback(async () => {
    if (!sessionId) {
      setSession(null);
      return;
    }
    try {
      const [active, archived] = await Promise.all([
        invoke<RecentSessionDto[]>("list_sessions", {
          filter: "active",
          limit: 200,
        }),
        invoke<RecentSessionDto[]>("list_sessions", {
          filter: "archived",
          limit: 200,
        }),
      ]);
      const match = [...(active ?? []), ...(archived ?? [])].find(
        (row) => row.sessionId === sessionId,
      );
      setSession(match?.summary?.trim() ? match : null);
    } catch {
      setSession(null);
    }
  }, [sessionId]);

  useEffect(() => {
    setSession(null);
    void load();
  }, [load]);

  useEffect(() => subscribeSessionsChanged(() => void load()), [load]);

  useEffect(() => {
    if (!sessionId) return;
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
      return;
    }
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<SessionEventPayload>("session_event", ({ payload }) => {
      if (payload.sessionId?.trim() !== sessionId) return;
      const next = payload.sessionMetadataChanged?.title?.trim();
      if (next) {
        setSession((current) =>
          current
            ? { ...current, summary: next }
            : { sessionId, source: "unknown", summary: next, createdAt: null },
        );
      }
    })
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [sessionId]);

  return session;
}

export function useActiveSessionTitle(sessionId: string | null): string | null {
  return useActiveSessionMetadata(sessionId)?.summary ?? null;
}
