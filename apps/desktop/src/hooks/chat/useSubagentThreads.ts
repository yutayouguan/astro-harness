import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export type SubagentThreadStatus =
  | "pending"
  | "running"
  | "completed"
  | "failed"
  | "interrupted"
  | "closed";

export type SubagentThread = {
  id: string;
  parent_session_id: string;
  agent_name: string;
  task: string;
  status: SubagentThreadStatus;
  summary?: string | null;
  error?: string | null;
  model?: string | null;
  model_reasoning_effort?: string | null;
  sandbox_mode?: string | null;
  created_at: string;
  updated_at: string;
};

export function useSubagentThreads(parentSessionId?: string | null) {
  const [threads, setThreads] = useState<SubagentThread[]>([]);
  const [error, setError] = useState<string | null>(null);
  const requestSeq = useRef(0);

  const refresh = useCallback(async () => {
    const sessionId = parentSessionId?.trim();
    if (!sessionId) {
      setThreads([]);
      setError(null);
      return;
    }
    const seq = ++requestSeq.current;
    try {
      const next = await invoke<SubagentThread[]>("list_subagent_threads", {
        args: { parentSessionId: sessionId, includeClosed: true },
      });
      if (seq !== requestSeq.current) return;
      setThreads(next);
      setError(null);
    } catch (reason) {
      if (seq !== requestSeq.current) return;
      setError(String(reason));
    }
  }, [parentSessionId]);

  useEffect(() => {
    requestSeq.current += 1;
    setThreads([]);
    setError(null);
    if (!parentSessionId) return;
    void refresh();
    const timer = window.setInterval(() => void refresh(), 1_500);
    return () => {
      window.clearInterval(timer);
      requestSeq.current += 1;
    };
  }, [parentSessionId, refresh]);

  return { threads, error, refresh };
}
