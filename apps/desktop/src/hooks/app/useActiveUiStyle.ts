import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";

import { readMorphiconPrefs } from "../../lib/ui/morphiconPrefs";
import { applyUiStyleTokenDiff } from "../../lib/ui/uiStyleTokenDiff";
import {
  tokensForResolvedTheme,
  type ActiveUiStyle,
} from "../../lib/ui/activeUiStyle";

const REFRESH_MS = 15_000;
export const UI_STYLE_CHANGED_EVENT = "ui-style-changed";
export const UI_STYLE_RESET_EVENT = "astro:ui-style-reset";

function isTauriRuntime(): boolean {
  return Boolean(
    (window as unknown as { __TAURI_INTERNALS__?: unknown })
      .__TAURI_INTERNALS__,
  );
}

function restoreIconPreferences(root: HTMLElement) {
  const prefs = readMorphiconPrefs();
  root.dataset.iconMotion = prefs.spring;
  root.style.setProperty("--app-icon-stroke-width", String(prefs.strokeWidth));
}

export function useActiveUiStyle() {
  const [style, setStyle] = useState<ActiveUiStyle | null>(null);
  const styleRef = useRef(style);
  styleRef.current = style;
  const [error, setError] = useState<string | null>(null);
  const appliedTokensRef = useRef<string[]>([]);
  const refreshSequence = useRef(0);

  const refresh = useCallback(async () => {
    if (!isTauriRuntime()) return;
    const sequence = ++refreshSequence.current;
    try {
      const next = await invoke<ActiveUiStyle | null>("get_active_ui_style");
      if (sequence !== refreshSequence.current) return;
      setStyle((current) =>
        current?.revision === next?.revision ? current : next,
      );
      setError(null);
    } catch (cause) {
      if (sequence !== refreshSequence.current) return;
      setStyle(null);
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  }, []);

  const reset = useCallback(async () => {
    refreshSequence.current++;
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
    const onReset = () => {
      refreshSequence.current++;
      setStyle(null);
    };
    void refresh();
    void listen(UI_STYLE_CHANGED_EVENT, () => void refresh())
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch(() => {});
    const timer = window.setInterval(onVisible, REFRESH_MS);
    window.addEventListener("focus", onVisible);
    window.addEventListener(UI_STYLE_RESET_EVENT, onReset);
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      disposed = true;
      refreshSequence.current++;
      unlisten?.();
      window.clearInterval(timer);
      window.removeEventListener("focus", onVisible);
      window.removeEventListener(UI_STYLE_RESET_EVENT, onReset);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, [refresh]);

  const apply = useCallback(() => {
    const root = document.documentElement;
    const style = styleRef.current;
    const theme = root.dataset.theme === "light" ? "light" : "dark";
    const tokens = tokensForResolvedTheme(style, theme);
    appliedTokensRef.current = applyUiStyleTokenDiff(
      root.style,
      appliedTokensRef.current,
      tokens,
    );
    if (style) {
      root.dataset.userUiStyle = style.id;
    } else {
      delete root.dataset.userUiStyle;
    }
    const iconPrefs = readMorphiconPrefs();
    const motion = style?.icons.motion ?? iconPrefs.spring;
    if (root.dataset.iconMotion !== motion) root.dataset.iconMotion = motion;
    const stroke = String(style?.icons.strokeWidth ?? iconPrefs.strokeWidth);
    if (root.style.getPropertyValue("--app-icon-stroke-width") !== stroke)
      root.style.setProperty("--app-icon-stroke-width", stroke);
  }, []);

  useLayoutEffect(() => {
    apply();
  }, [style, apply]);

  useLayoutEffect(() => {
    const root = document.documentElement;
    const observer = new MutationObserver((records) => {
      if (records.some((record) => record.attributeName === "data-theme")) {
        apply();
      }
    });
    observer.observe(root, {
      attributes: true,
      attributeFilter: ["data-theme"],
    });
    return () => {
      observer.disconnect();
      for (const token of appliedTokensRef.current) {
        root.style.removeProperty(token);
      }
      appliedTokensRef.current = [];
      delete root.dataset.userUiStyle;
      restoreIconPreferences(root);
    };
  }, [apply]);

  return { style, error, refresh, reset };
}
