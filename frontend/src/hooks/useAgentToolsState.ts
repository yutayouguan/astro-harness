/**
 * 内置工具集启用状态：Tauri 下走 `get/set_tools_enabled`，浏览器预览回退 localStorage。
 */

import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AgentToolId } from "./useAgentTools";
import { AGENT_TOOLS } from "./useAgentTools";

const isTauri = () =>
  typeof window !== "undefined" && !!(window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;

function defaultEnabled(): Record<AgentToolId, boolean> {
  return Object.fromEntries(
    AGENT_TOOLS.map((tool) => [tool.id, true]),
  ) as Record<AgentToolId, boolean>;
}

function mergeEnabled(
  stored: Record<string, boolean> | null | undefined,
): Record<AgentToolId, boolean> {
  const base = defaultEnabled();
  if (!stored) return base;
  for (const tool of AGENT_TOOLS) {
    if (typeof stored[tool.id] === "boolean") {
      base[tool.id] = stored[tool.id]!;
    }
  }
  return base;
}

/** @returns `enabled` 映射与 toggle / setToolEnabled */
export function useAgentTools() {
  const [enabled, setEnabled] = useState<Record<AgentToolId, boolean>>(defaultEnabled);
  const [ready, setReady] = useState(false);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      if (!isTauri()) {
        // 浏览器预览：回退 localStorage
        try {
          const raw = localStorage.getItem("agent-tools");
          if (!cancelled) {
            setEnabled(mergeEnabled(raw ? JSON.parse(raw) : null));
            setReady(true);
          }
        } catch {
          if (!cancelled) setReady(true);
        }
        return;
      }
      try {
        const stored = await invoke<Record<string, boolean>>("get_tools_enabled");
        if (!cancelled) {
          setEnabled(mergeEnabled(stored));
          setReady(true);
        }
      } catch {
        if (!cancelled) setReady(true);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (!ready) return;
    if (!isTauri()) {
      try {
        localStorage.setItem("agent-tools", JSON.stringify(enabled));
      } catch {
        // ignore
      }
      return;
    }
    void invoke("set_tools_enabled", { enabled }).catch(() => {});
  }, [enabled, ready]);

  const toggle = useCallback((id: AgentToolId) => {
    setEnabled((prev) => ({ ...prev, [id]: !prev[id] }));
  }, []);

  const setToolEnabled = useCallback((id: AgentToolId, value: boolean) => {
    setEnabled((prev) => ({ ...prev, [id]: value }));
  }, []);

  return { enabled, toggle, setToolEnabled };
}
