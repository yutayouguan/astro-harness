/** Agent 列表变更事件总线。 */
import { useEffect, useRef } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** 与 Rust `EVENT_AGENTS_CHANGED` 对齐 */
export const AGENTS_CHANGED_EVENT = "agents-changed";

export type AgentsChangedPayload = {
  active_agent_id: string;
};

function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** 订阅 Agent 列表 / 激活 Agent 变更（create_agent、手动切换等） */
export async function listenAgentsChanged(
  onChange: (payload: AgentsChangedPayload) => void,
): Promise<UnlistenFn> {
  if (!isTauri()) return () => {};
  return listen<AgentsChangedPayload>(AGENTS_CHANGED_EVENT, (event) => {
    onChange(event.payload);
  });
}

/** React：在 Agent 变更时回调（适合刷新 get_config / 切换器） */
export function useAgentsChanged(
  onChange: (payload: AgentsChangedPayload) => void,
): void {
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;
    void listenAgentsChanged((payload) => {
      if (!cancelled) onChangeRef.current(payload);
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);
}
