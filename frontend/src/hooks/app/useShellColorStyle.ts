/** Shell 色彩风格偏好：多彩 / 统一 + 渐变（含预览事务）。 */
import { useCallback, useRef, useState } from "react";
import {
  cloneGradient,
  DEFAULT_SHELL_COLOR_PREFS,
  type ShellColorPrefs,
  type ShellColorStyle,
  type ShellGradient,
  normalizeGradient,
} from "../../lib/ui/shellGradient";

const STORAGE_KEY_V2 = "astro-shell-color-prefs.v2";
const STORAGE_KEY_V1 = "astro-shell-color-style";

function readStoredPrefs(): ShellColorPrefs {
  try {
    const raw = localStorage.getItem(STORAGE_KEY_V2);
    if (raw) {
      const parsed = JSON.parse(raw) as unknown;
      if (parsed && typeof parsed === "object") {
        const o = parsed as Record<string, unknown>;
        const style =
          o.style === "unified" || o.style === "colorful"
            ? (o.style as ShellColorStyle)
            : "colorful";
        return {
          style,
          gradient: normalizeGradient(o.gradient),
        };
      }
    }
  } catch {
    // ignore
  }

  // 迁移 v1：仅 style 字符串
  try {
    const v1 = localStorage.getItem(STORAGE_KEY_V1);
    if (v1 === "unified" || v1 === "colorful") {
      return {
        style: v1,
        gradient: cloneGradient(DEFAULT_SHELL_COLOR_PREFS.gradient),
      };
    }
  } catch {
    // ignore
  }

  return {
    style: DEFAULT_SHELL_COLOR_PREFS.style,
    gradient: cloneGradient(DEFAULT_SHELL_COLOR_PREFS.gradient),
  };
}

function persistPrefs(prefs: ShellColorPrefs) {
  try {
    localStorage.setItem(
      STORAGE_KEY_V2,
      JSON.stringify({
        style: prefs.style,
        gradient: prefs.gradient,
      }),
    );
    // 保留 v1 键，方便旧代码/调试读到最新 style
    localStorage.setItem(STORAGE_KEY_V1, prefs.style);
  } catch {
    // ignore
  }
}

export type { ShellColorStyle } from "../../lib/ui/shellGradient";

export function useShellColorStyle() {
  const [prefs, setPrefsState] = useState<ShellColorPrefs>(() =>
    typeof window === "undefined" ? DEFAULT_SHELL_COLOR_PREFS : readStoredPrefs(),
  );
  const snapshotRef = useRef<ShellColorPrefs | null>(null);
  const [editing, setEditing] = useState(false);

  const setColorStyle = useCallback((style: ShellColorStyle) => {
    setPrefsState((prev) => {
      const next = { ...prev, style };
      persistPrefs(next);
      return next;
    });
  }, []);

  /** 立即持久化（预设色） */
  const setGradient = useCallback((gradient: ShellGradient) => {
    setPrefsState((prev) => {
      const next = { ...prev, style: "unified" as const, gradient: cloneGradient(gradient) };
      persistPrefs(next);
      return next;
    });
  }, []);

  /** 打开自定义编辑：拍快照，之后仅预览 */
  const beginGradientEdit = useCallback(() => {
    setPrefsState((prev) => {
      snapshotRef.current = {
        style: prev.style,
        gradient: cloneGradient(prev.gradient),
      };
      return {
        ...prev,
        style: "unified",
        gradient: {
          ...cloneGradient(prev.gradient),
          id: "custom",
        },
      };
    });
    setEditing(true);
  }, []);

  /** 编辑中实时预览，不写盘 */
  const previewGradient = useCallback((gradient: ShellGradient) => {
    setPrefsState((prev) => ({
      ...prev,
      style: "unified",
      gradient: cloneGradient(gradient),
    }));
  }, []);

  const commitGradientEdit = useCallback((gradient?: ShellGradient) => {
    setPrefsState((prev) => {
      const base = gradient ? cloneGradient(gradient) : cloneGradient(prev.gradient);
      const next = {
        style: "unified" as const,
        gradient: { ...base, id: "custom" as const },
      };
      persistPrefs(next);
      return next;
    });
    snapshotRef.current = null;
    setEditing(false);
  }, []);

  const cancelGradientEdit = useCallback(() => {
    const snap = snapshotRef.current;
    if (snap) {
      setPrefsState({
        style: snap.style,
        gradient: cloneGradient(snap.gradient),
      });
      // 若打开前已是 unified，取消不应丢弃已保存的预设；快照本身就是打开前状态
      // 快照未 persist（打开时没写盘），恢复内存即可
    }
    snapshotRef.current = null;
    setEditing(false);
  }, []);

  return {
    colorStyle: prefs.style,
    gradient: prefs.gradient,
    setColorStyle,
    setGradient,
    beginGradientEdit,
    previewGradient,
    commitGradientEdit,
    cancelGradientEdit,
    editingGradient: editing,
  };
}
