import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  EMPTY_AGENT_TREE,
  flattenAgentTree,
  fromSnapshotWithBufferedEvents,
  markThreadRead,
  normalizeAgentThreadChanged,
  normalizeAgentTreeSnapshot,
  reduceAgentThreadEvent,
  type AgentThreadChanged,
  type BufferedAgentThreadChanged,
  type AgentTreeState,
} from "./subagentTree";

export type SessionAgentThreadEvent = {
  sessionId?: string | null;
  eventId: number;
  streamId: string;
  agentThreadChanged?: unknown | null;
  resyncRequired?: { reason: string } | null;
};

function visibleTreeRoots(state: AgentTreeState) {
  return state.roots.flatMap((node) =>
    node.thread.canonicalPath === "/root" && node.thread.parentThreadId === null
      ? node.children
      : [node]);
}

export function useSubagentThreads(rootSessionId?: string | null) {
  const [state, setStateValue] = useState<AgentTreeState>(EMPTY_AGENT_TREE);
  const [error, setError] = useState<string | null>(null);
  const stateRef = useRef(state);
  const lifecycleRef = useRef(0);
  const refreshRef = useRef(0);
  const bufferingRef = useRef(false);
  const bufferedRef = useRef<BufferedAgentThreadChanged[]>([]);
  const desiredStreamRef = useRef<string | null>(null);

  const commitState = useCallback((next: AgentTreeState) => {
    stateRef.current = next;
    setStateValue(next);
  }, []);

  const refresh = useCallback(async () => {
    const root = rootSessionId?.trim();
    if (!root) {
      commitState(EMPTY_AGENT_TREE);
      setError(null);
      return;
    }

    const lifecycle = lifecycleRef.current;
    const request = ++refreshRef.current;
    bufferingRef.current = true;
    try {
      const raw = await invoke<unknown>("list_subagent_threads", {
        args: { rootSessionId: root },
      });
      if (lifecycle !== lifecycleRef.current || request !== refreshRef.current) return;

      const snapshot = normalizeAgentTreeSnapshot(raw);
      if (snapshot.rootThreadId !== root) {
        throw new Error("agent tree snapshot belongs to another root session");
      }
      const previous = stateRef.current.rootThreadId === root
        ? stateRef.current
        : EMPTY_AGENT_TREE;
      const streamId = desiredStreamRef.current;
      const next = fromSnapshotWithBufferedEvents(
        snapshot,
        previous,
        bufferedRef.current,
        streamId,
      );

      bufferedRef.current = [];
      bufferingRef.current = false;
      commitState(next);
      setError(null);
    } catch (reason) {
      if (lifecycle !== lifecycleRef.current || request !== refreshRef.current) return;
      // A failed baseline must not let later deltas build an incomplete tree.
      // Keep buffering until a manual or automatic refresh succeeds.
      bufferingRef.current = true;
      setError(String(reason));
    }
  }, [commitState, rootSessionId]);

  useEffect(() => {
    const root = rootSessionId?.trim();
    const lifecycle = ++lifecycleRef.current;
    refreshRef.current += 1;
    bufferedRef.current = [];
    desiredStreamRef.current = null;
    bufferingRef.current = Boolean(root);
    commitState(root ? { ...EMPTY_AGENT_TREE, rootThreadId: root } : EMPTY_AGENT_TREE);
    setError(null);
    if (!root) return;

    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<SessionAgentThreadEvent>("session_event", (event) => {
      if (disposed || lifecycle !== lifecycleRef.current) return;
      const payload = event.payload;
      if (payload.sessionId?.trim() !== root) return;

      const streamId = payload.streamId ?? "";
      const previousDesired = desiredStreamRef.current;
      if (previousDesired == null || previousDesired !== streamId) {
        desiredStreamRef.current = streamId;
      }
      const streamChanged = previousDesired != null && previousDesired !== streamId;
      let changed: AgentThreadChanged | null = null;
      if (payload.agentThreadChanged) {
        try {
          changed = normalizeAgentThreadChanged(payload.agentThreadChanged);
        } catch (reason) {
          setError(String(reason));
          void refresh();
          return;
        }
        if (changed.rootThreadId !== root) return;
      }

      if (changed && (bufferingRef.current || streamChanged || payload.resyncRequired)) {
        bufferedRef.current.push({ streamId, changed });
      }

      // Field 14 is an explicit baseline invalidation even when streamId is
      // unchanged. A generation change likewise makes the current cursor
      // incomparable until the fresh snapshot resolves.
      if (payload.resyncRequired || streamChanged) {
        void refresh();
        return;
      }
      if (!changed || bufferingRef.current) return;

      const next = reduceAgentThreadEvent(stateRef.current, changed);
      if (next !== stateRef.current) commitState(next);
    })
      .then((stop) => {
        if (disposed || lifecycle !== lifecycleRef.current) stop();
        else {
          unlisten = stop;
          // Listener-first startup closes the snapshot/event race.
          void refresh();
        }
      })
      .catch((reason) => {
        if (disposed || lifecycle !== lifecycleRef.current) return;
        setError(String(reason));
        // The one-shot snapshot is still useful if event registration fails.
        void refresh();
      });

    return () => {
      disposed = true;
      lifecycleRef.current += 1;
      refreshRef.current += 1;
      unlisten?.();
    };
  }, [commitState, refresh, rootSessionId]);

  const markRead = useCallback((path: string) => {
    const next = markThreadRead(stateRef.current, path);
    if (next !== stateRef.current) commitState(next);
  }, [commitState]);

  const roots = useMemo(() => visibleTreeRoots(state), [state]);
  const threads = useMemo(
    () => flattenAgentTree(roots).map((node) => node.thread),
    [roots],
  );
  return { state, threads, roots, error, refresh, markRead };
}

export type {
  AgentThread,
  AgentThreadStatus,
  AgentTreeNode,
  AgentTreeState,
} from "./subagentTree";
