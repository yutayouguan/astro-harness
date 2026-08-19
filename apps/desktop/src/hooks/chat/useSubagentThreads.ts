import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  EMPTY_AGENT_TREE,
  classifyAgentThreadSessionEvent,
  flattenAgentTree,
  fromSnapshotWithBufferedEvents,
  isAgentTreeGenerationCurrent,
  isAgentTreeRequestCurrent,
  markThreadRead,
  normalizeAgentTreeSnapshot,
  reduceAgentThreadEvent,
  type AgentThreadSessionEventPayload,
  type AgentTreeGenerationToken,
  type AgentTreeRequestTicket,
  type BufferedAgentThreadChanged,
  type AgentTreeState,
} from "./subagentTree";

export type SessionAgentThreadEvent = AgentThreadSessionEventPayload & {
  eventId: number;
};

function emptyStateFor(root: string): AgentTreeState {
  return root ? { ...EMPTY_AGENT_TREE, rootThreadId: root } : EMPTY_AGENT_TREE;
}

function visibleTreeRoots(state: AgentTreeState) {
  return state.roots.flatMap((node) =>
    node.thread.canonicalPath === "/root" && node.thread.parentThreadId === null
      ? node.children
      : [node]);
}

export function useSubagentThreads(rootSessionId?: string | null) {
  const root = rootSessionId?.trim() ?? "";
  const activeRootRef = useRef(root);
  const generationRef = useRef(0);
  // Root identity changes during render, before old callbacks or effects can
  // run. This closes the render-to-effect window for stale A -> B completions.
  if (activeRootRef.current !== root) {
    activeRootRef.current = root;
    generationRef.current += 1;
  }

  const [state, setStateValue] = useState<AgentTreeState>(() => emptyStateFor(root));
  const [error, setError] = useState<string | null>(null);
  const [loadingState, setLoadingState] = useState(Boolean(root));
  const [initializedState, setInitializedState] = useState(false);
  const stateRef = useRef(state);
  const refreshRef = useRef(0);
  const bufferingRef = useRef(Boolean(root));
  const bufferedRef = useRef<BufferedAgentThreadChanged[]>([]);
  const desiredStreamRef = useRef<string | null>(null);

  const commitState = useCallback((next: AgentTreeState) => {
    stateRef.current = next;
    setStateValue(next);
  }, []);

  const refresh = useCallback(async () => {
    if (!root) {
      if (activeRootRef.current === root) {
        commitState(EMPTY_AGENT_TREE);
        setError(null);
        setLoadingState(false);
        setInitializedState(false);
      }
      return;
    }
    if (activeRootRef.current !== root) return;

    const request = ++refreshRef.current;
    const ticket: AgentTreeRequestTicket = {
      root,
      generation: generationRef.current,
      request,
    };
    if (!isAgentTreeRequestCurrent(
      ticket,
      activeRootRef.current,
      generationRef.current,
      refreshRef.current,
    )) return;

    bufferingRef.current = true;
    setLoadingState(true);
    try {
      // Validate once more directly before crossing the async boundary.
      if (!isAgentTreeRequestCurrent(
        ticket,
        activeRootRef.current,
        generationRef.current,
        refreshRef.current,
      )) return;
      const raw = await invoke<unknown>("list_subagent_threads", {
        args: { rootSessionId: root },
      });
      if (!isAgentTreeRequestCurrent(
        ticket,
        activeRootRef.current,
        generationRef.current,
        refreshRef.current,
      )) return;

      const snapshot = normalizeAgentTreeSnapshot(raw);
      if (snapshot.rootThreadId !== root) {
        throw new Error("agent tree snapshot belongs to another root session");
      }
      const previous = stateRef.current.rootThreadId === root
        ? stateRef.current
        : EMPTY_AGENT_TREE;
      const next = fromSnapshotWithBufferedEvents(
        snapshot,
        previous,
        bufferedRef.current,
        desiredStreamRef.current,
      );
      if (!isAgentTreeRequestCurrent(
        ticket,
        activeRootRef.current,
        generationRef.current,
        refreshRef.current,
      )) return;

      bufferedRef.current = [];
      bufferingRef.current = false;
      commitState(next);
      setError(null);
      setLoadingState(false);
      setInitializedState(true);
    } catch (reason) {
      if (!isAgentTreeRequestCurrent(
        ticket,
        activeRootRef.current,
        generationRef.current,
        refreshRef.current,
      )) return;
      // A failed baseline must not let later deltas build an incomplete tree.
      bufferingRef.current = true;
      setError(String(reason));
      setLoadingState(false);
    }
  }, [commitState, root]);

  useEffect(() => {
    const token: AgentTreeGenerationToken = {
      root,
      generation: generationRef.current,
    };
    refreshRef.current += 1;
    bufferedRef.current = [];
    desiredStreamRef.current = null;
    bufferingRef.current = Boolean(root);
    commitState(emptyStateFor(root));
    setError(null);
    setLoadingState(Boolean(root));
    setInitializedState(false);
    if (!root) return;

    let disposed = false;
    let unlisten: (() => void) | undefined;
    const isCurrent = () => !disposed && isAgentTreeGenerationCurrent(
      token,
      activeRootRef.current,
      generationRef.current,
    );

    void listen<SessionAgentThreadEvent>("session_event", (event) => {
      if (!isCurrent()) return;

      let classification;
      try {
        classification = classifyAgentThreadSessionEvent(
          event.payload,
          root,
          desiredStreamRef.current,
        );
      } catch (reason) {
        if (!isCurrent()) return;
        setError(String(reason));
        void refresh();
        return;
      }
      // Ignore unrelated memory/title/local events before touching any Agent
      // Tree stream-generation state.
      if (classification.kind === "ignore") return;
      desiredStreamRef.current = classification.nextStreamId;

      if (classification.kind === "refresh") {
        if (classification.changed) {
          bufferedRef.current.push({
            streamId: classification.nextStreamId,
            changed: classification.changed,
          });
        }
        void refresh();
        return;
      }

      if (bufferingRef.current) {
        bufferedRef.current.push({
          streamId: classification.nextStreamId,
          changed: classification.changed,
        });
        return;
      }
      const next = reduceAgentThreadEvent(stateRef.current, classification.changed);
      if (next !== stateRef.current && isCurrent()) commitState(next);
    })
      .then((stop) => {
        if (!isCurrent()) stop();
        else {
          unlisten = stop;
          // Listener-first startup closes the snapshot/event race.
          void refresh();
        }
      })
      .catch((reason) => {
        if (!isCurrent()) return;
        setError(String(reason));
        // The one-shot snapshot is still useful if event registration fails.
        void refresh();
      });

    return () => {
      disposed = true;
      refreshRef.current += 1;
      if (isAgentTreeGenerationCurrent(
        token,
        activeRootRef.current,
        generationRef.current,
      )) {
        generationRef.current += 1;
      }
      unlisten?.();
    };
  }, [commitState, refresh, root]);

  const markRead = useCallback((path: string) => {
    const currentRoot = activeRootRef.current;
    if (!currentRoot || stateRef.current.rootThreadId !== currentRoot) return;
    const next = markThreadRead(stateRef.current, path);
    if (next !== stateRef.current) commitState(next);
  }, [commitState]);

  const projectedState = state.rootThreadId === root
    ? state
    : emptyStateFor(root);
  const initialized = projectedState === state && initializedState;
  const loading = Boolean(root) && (projectedState !== state || loadingState);
  const roots = useMemo(() => visibleTreeRoots(projectedState), [projectedState]);
  const threads = useMemo(
    () => flattenAgentTree(roots).map((node) => node.thread),
    [roots],
  );
  return {
    state: projectedState,
    threads,
    roots,
    error: projectedState === state ? error : null,
    loading,
    initialized,
    refresh,
    markRead,
  };
}

export type {
  AgentThread,
  AgentThreadStatus,
  AgentTreeNode,
  AgentTreeState,
} from "./subagentTree";
