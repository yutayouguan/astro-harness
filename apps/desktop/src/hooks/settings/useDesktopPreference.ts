import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
} from "react";
import { invoke } from "@tauri-apps/api/core";

export type DesktopPreference = "notifications" | "autostart";
export const DesktopPreferencePreviewContext = createContext<{
  values: Record<DesktopPreference, boolean>;
  change: (kind: DesktopPreference, value: boolean) => void;
} | null>(null);
const commands = {
  notifications: [
    "get_task_notifications_enabled",
    "set_task_notifications_enabled",
  ],
  autostart: ["get_desktop_autostart", "set_desktop_autostart"],
} as const;

export function useDesktopPreference(kind: DesktopPreference, preview = false) {
  const previewState = useContext(DesktopPreferencePreviewContext);
  const [value, setValue] = useState<boolean | null>(preview ? false : null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const mounted = useRef(false);
  const [get, set] = commands[kind];
  const reload = useCallback(async () => {
    if (preview) return;
    setBusy(true);
    setError(false);
    try {
      const next = await invoke<boolean>(get);
      if (mounted.current) setValue(next);
    } catch {
      if (mounted.current) setError(true);
    } finally {
      if (mounted.current) setBusy(false);
    }
  }, [get, preview]);
  useEffect(() => {
    mounted.current = true;
    void reload();
    return () => {
      mounted.current = false;
    };
  }, [reload]);
  const change = async (enabled: boolean) => {
    if (busy || value === null) return;
    if (preview) {
      if (previewState) previewState.change(kind, enabled);
      else setValue(enabled);
      return;
    }
    setBusy(true);
    setError(false);
    try {
      const actual = await invoke<boolean>(set, { enabled });
      if (mounted.current) setValue(actual);
    } catch {
      if (mounted.current) setError(true);
      // The OS is authoritative even if a partial change/rollback failed.
      try {
        const actual = await invoke<boolean>(get);
        if (mounted.current) setValue(actual);
      } catch {
        if (mounted.current) setValue(null);
      }
    } finally {
      if (mounted.current) setBusy(false);
    }
  };
  return {
    value: preview && previewState ? previewState.values[kind] : value,
    busy,
    error,
    reload,
    change,
  };
}
