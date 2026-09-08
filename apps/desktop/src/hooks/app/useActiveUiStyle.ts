import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";

import { readMorphiconPrefs } from "../../lib/ui/morphiconPrefs";
import {
  tokensForResolvedTheme,
  type ActiveUiStyle,
} from "../../lib/ui/activeUiStyle";

const REFRESH_MS = 15_000;
export const UI_STYLE_CHANGED_EVENT = "ui-style-changed";
export const UI_STYLE_RESET_EVENT = "astro:ui-style-reset";

function isTauriRuntime(): boolean {
  return Boolean(
    (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__,
  );
}

function restoreIconPreferences(root: HTMLElement) {
  const prefs = readMorphiconPrefs();
  root.dataset.iconMotion = prefs.spring;
  root.style.setProperty("--app-icon-stroke-width", String(prefs.strokeWidth));
}

export function useActiveUiStyle() {
  const [style, setStyle] = useState<ActiveUiStyle | null>(null);
  const [error, setError] = useState<string | null>(null);
  const appliedTokensRef = useRef<string[]>([]);

  const refresh = useCallback(async () => {
    if (!isTauriRuntime()) return;
    try {
      const next = await invoke<ActiveUiStyle | null>("get_active_ui_style");
      setStyle((current) =>
        current?.revision === next?.revision ? current : next,
      );
      setError(null);
    } catch (cause) {
      setStyle(null);
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  }, []);

  const reset = useCallback(async () => {
    setStyle(null);
    window.dispatchEvent(new Event(UI_STYLE_RESET_EVENT));
    if (!isTauriRuntime()) return;
    await invoke("reset_active_ui_style");
  }, []);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const onVisible = () => {
      if (document.visibilityState === "visible") void refresh();
    };
    const onReset = () => setStyle(null);
    void refresh();
    void listen(UI_STYLE_CHANGED_EVENT, () => void refresh()).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    }).catch(() => {});
    const timer = window.setInterval(onVisible, REFRESH_MS);
    window.addEventListener("focus", onVisible);
    window.addEventListener(UI_STYLE_RESET_EVENT, onReset);
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      disposed = true;
      unlisten?.();
      window.clearInterval(timer);
      window.removeEventListener("focus", onVisible);
      window.removeEventListener(UI_STYLE_RESET_EVENT, onReset);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, [refresh]);

  useLayoutEffect(() => {
    const root = document.documentElement;
    const apply = () => {
      for (const token of appliedTokensRef.current) {
        root.style.removeProperty(token);
      }
      const theme = root.dataset.theme === "light" ? "light" : "dark";
      const tokens = tokensForResolvedTheme(style, theme);
      appliedTokensRef.current = Object.keys(tokens);
      for (const [token, value] of Object.entries(tokens)) {
        root.style.setProperty(token, value);
      }
      if (style) {
        root.dataset.userUiStyle = style.id;
      } else {
        delete root.dataset.userUiStyle;
      }
      const iconPrefs = readMorphiconPrefs();
      root.dataset.iconMotion = style?.icons.motion ?? iconPrefs.spring;
      root.style.setProperty(
        "--app-icon-stroke-width",
        String(style?.icons.strokeWidth ?? iconPrefs.strokeWidth),
      );
    };

    apply();
    const observer = new MutationObserver((records) => {
      if (records.some((record) => record.attributeName === "data-theme")) {
        apply();
      }
    });
    observer.observe(root, { attributes: true, attributeFilter: ["data-theme"] });
    return () => {
      observer.disconnect();
      for (const token of appliedTokensRef.current) {
        root.style.removeProperty(token);
      }
      appliedTokensRef.current = [];
      delete root.dataset.userUiStyle;
      restoreIconPreferences(root);
    };
  }, [style]);

  return { style, error, refresh, reset };
}
