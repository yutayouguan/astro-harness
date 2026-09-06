import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  EMPTY_AGENT_TREE,
  classifyAgentThreadSessionEvent,
  createAgentTreeRootLifecycle,
  flattenAgentTree,
  fromSnapshotWithBufferedEvents,
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
      : [node],
  );
}

export function useSubagentThreads(rootSessionId?: string | null) {
  const root = rootSessionId?.trim() ?? "";
  const [rootLifecycle] = useState(createAgentTreeRootLifecycle);

  const [state, setStateValue] = useState<AgentTreeState>(() =>
    emptyStateFor(root),
  );
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
      if (rootLifecycle.current().root === root) {
        commitState(EMPTY_AGENT_TREE);
        setError(null);
        setLoadingState(false);
        setInitializedState(false);
      }
      return;
    }
    const active = rootLifecycle.current();
    if (active.root !== root) return;

    const request = ++refreshRef.current;
    const ticket: AgentTreeRequestTicket = {
      root,
      generation: active.generation,
      request,
    };
    const isCurrentRequest = () => {
      const current = rootLifecycle.current();
      return isAgentTreeRequestCurrent(
        ticket,
        current.root,
        current.generation,
        refreshRef.current,
      );
    };
    if (!isCurrentRequest()) return;

    bufferingRef.current = true;
    setLoadingState(true);
    try {
      // Validate once more directly before crossing the async boundary.
      if (!isCurrentRequest()) return;
      const raw = await invoke<unknown>("list_subagent_threads", {
        args: { rootSessionId: root },
      });
      if (!isCurrentRequest()) return;

      const snapshot = normalizeAgentTreeSnapshot(raw);
      if (snapshot.rootThreadId !== root) {
        throw new Error("agent tree snapshot belongs to another root session");
      }
      const previous =
        stateRef.current.rootThreadId === root
          ? stateRef.current
          : EMPTY_AGENT_TREE;
      const next = fromSnapshotWithBufferedEvents(
        snapshot,
        previous,
        bufferedRef.current,
        desiredStreamRef.current,
      );
      if (!isCurrentRequest()) return;

      bufferedRef.current = [];
      bufferingRef.current = false;
      commitState(next);
      setError(null);
      setLoadingState(false);
      setInitializedState(true);
    } catch (reason) {
      if (!isCurrentRequest()) return;
      // A failed baseline must not let later deltas build an incomplete tree.
      bufferingRef.current = true;
      setError(String(reason));
      setLoadingState(false);
    }
  }, [commitState, root, rootLifecycle]);

  useLayoutEffect(() => {
    const token = rootLifecycle.commit(root);
    refreshRef.current += 1;
    bufferedRef.current = [];
    desiredStreamRef.current = null;
    bufferingRef.current = Boolean(root);
    commitState(emptyStateFor(root));
    setError(null);
    setLoadingState(Boolean(root));
    setInitializedState(false);
    return () => {
      rootLifecycle.invalidate(token);
      refreshRef.current += 1;
    };
  }, [commitState, root, rootLifecycle]);

  useEffect(() => {
    if (!root) return;

    const token: AgentTreeGenerationToken = rootLifecycle.current();
    if (token.root !== root) return;

    let disposed = false;
    let unlisten: (() => void) | undefined;
    let retryTimer: ReturnType<typeof globalThis.setTimeout> | undefined;
    const isCurrent = () => !disposed && rootLifecycle.isCurrent(token);

    const handleEvent = (event: { payload: SessionAgentThreadEvent }) => {
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
      const next = reduceAgentThreadEvent(
        stateRef.current,
        classification.changed,
      );
      if (next !== stateRef.current && isCurrent()) commitState(next);
    };

    const registerListener = () => {
      if (!isCurrent()) return;
      void listen<SessionAgentThreadEvent>("session_event", handleEvent)
        .then((stop) => {
          if (!isCurrent()) stop();
          else {
            unlisten = stop;
            // Listener-first startup/recovery closes the snapshot/event race.
            void refresh();
          }
        })
        .catch((reason) => {
          if (!isCurrent()) return;
          setError(String(reason));
          // Keep a one-shot baseline visible while retrying the live channel.
          void refresh();
          retryTimer = globalThis.setTimeout(registerListener, 1_000);
        });
    };

    registerListener();

    return () => {
      disposed = true;
      refreshRef.current += 1;
      if (retryTimer !== undefined) globalThis.clearTimeout(retryTimer);
      unlisten?.();
    };
  }, [commitState, refresh, root, rootLifecycle]);

  const markRead = useCallback(
    (path: string) => {
      const currentRoot = rootLifecycle.current().root;
      if (!currentRoot || stateRef.current.rootThreadId !== currentRoot) return;
      const next = markThreadRead(stateRef.current, path);
      if (next !== stateRef.current) commitState(next);
    },
    [commitState, rootLifecycle],
  );

  const projectedState =
    state.rootThreadId === root ? state : emptyStateFor(root);
  const initialized = projectedState === state && initializedState;
  const loading = Boolean(root) && (projectedState !== state || loadingState);
  const roots = useMemo(
    () => visibleTreeRoots(projectedState),
    [projectedState],
  );
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
