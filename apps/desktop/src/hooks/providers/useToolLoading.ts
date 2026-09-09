import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type {
  ToolLoadingMode,
  ToolLoadingSettings,
} from "../../lib/tools/toolLoading";

/** Global preferences: save one group only and never persist on initial load. */
export function useToolLoading(active: boolean) {
  const [settings, setSettings] = useState<ToolLoadingSettings | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const busy = useRef(false);
  const mounted = useRef(true);
  const revision = useRef(0);

  const refresh = useCallback(async () => {
    if (busy.current) return;
    const request = ++revision.current;
    try {
      const result = await invoke<ToolLoadingSettings>(
        "get_tool_loading_settings",
      );
      if (mounted.current && request === revision.current) {
        setSettings(result);
        setError(null);
      }
    } catch (cause) {
      if (mounted.current && request === revision.current) {
        setSettings(null);
        setError(String(cause));
      }
    }
  }, []);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      revision.current++;
    };
  }, []);

  useEffect(() => {
    if (active && "__TAURI_INTERNALS__" in window) void refresh();
  }, [active, refresh]);

  const change = useCallback(async (toolset: string, mode: ToolLoadingMode) => {
    if (busy.current) return;
    busy.current = true;
    setSaving(true);
    ++revision.current;
    try {
      const result = await invoke<ToolLoadingSettings>(
        "set_tool_loading_mode",
        { toolset, mode },
      );
      if (mounted.current) {
        setSettings(result);
        setError(null);
      }
    } catch (cause) {
      // Keep the last confirmed selection; a failed save must not look applied.
      if (mounted.current) setError(String(cause));
    } finally {
      busy.current = false;
      if (mounted.current) setSaving(false);
    }
  }, []);

  return { settings, error, saving, refresh, change };
}
