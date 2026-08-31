/** Shell 色彩风格偏好：多彩 / 统一 / 灵动（含预览事务）。 */
import { useCallback, useRef, useState } from "react";
import { createDynamicSeed } from "../../lib/ui/dynamicGradient";
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

function parseStyle(raw: unknown): ShellColorStyle {
  if (raw === "unified" || raw === "colorful" || raw === "dynamic") return raw;
  return DEFAULT_SHELL_COLOR_PREFS.style;
}

function readStoredPrefs(): ShellColorPrefs {
  try {
    const raw = localStorage.getItem(STORAGE_KEY_V2);
    if (raw) {
      const parsed = JSON.parse(raw) as unknown;
      if (parsed && typeof parsed === "object") {
        const o = parsed as Record<string, unknown>;
        const seed =
          typeof o.dynamicSeed === "string" && o.dynamicSeed.trim()
            ? o.dynamicSeed.trim()
            : createDynamicSeed();
        return {
          style: parseStyle(o.style),
          gradient: normalizeGradient(o.gradient),
          dynamicSeed: seed,
        };
      }
    }
  } catch {
    // ignore
  }

  // 迁移 v1：仅 style 字符串
  try {
    const v1 = localStorage.getItem(STORAGE_KEY_V1);
    if (v1 === "unified" || v1 === "colorful" || v1 === "dynamic") {
      return {
        style: v1,
        gradient: cloneGradient(DEFAULT_SHELL_COLOR_PREFS.gradient),
        dynamicSeed: createDynamicSeed(),
      };
    }
  } catch {
    // ignore
  }

  return {
    style: DEFAULT_SHELL_COLOR_PREFS.style,
    gradient: cloneGradient(DEFAULT_SHELL_COLOR_PREFS.gradient),
    dynamicSeed: createDynamicSeed(),
  };
}

function persistPrefs(prefs: ShellColorPrefs) {
  try {
    localStorage.setItem(
      STORAGE_KEY_V2,
      JSON.stringify({
        style: prefs.style,
        gradient: prefs.gradient,
        dynamicSeed: prefs.dynamicSeed,
      }),
    );
    localStorage.setItem(STORAGE_KEY_V1, prefs.style);
  } catch {
    // ignore
  }
}

export type { ShellColorStyle } from "../../lib/ui/shellGradient";

export function useShellColorStyle() {
  const [prefs, setPrefsState] = useState<ShellColorPrefs>(() =>
    typeof window === "undefined"
      ? DEFAULT_SHELL_COLOR_PREFS
      : readStoredPrefs(),
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
      const next = {
        ...prev,
        style: "unified" as const,
        gradient: cloneGradient(gradient),
      };
      persistPrefs(next);
      return next;
    });
  }, []);

  /** 重新生成灵动配色种子（全体 Tab 换色） */
  const reshuffleDynamic = useCallback(() => {
    const dynamicSeed = createDynamicSeed();
    setPrefsState((prev) => {
      const next = {
        ...prev,
        style: "dynamic" as const,
        dynamicSeed,
      };
      // 写盘放到微任务，避免阻塞本次渲染与配色同步
      queueMicrotask(() => persistPrefs(next));
      return next;
    });
  }, []);

  /** 打开自定义编辑：拍快照，之后仅预览 */
  const beginGradientEdit = useCallback(() => {
    setPrefsState((prev) => {
      snapshotRef.current = {
        style: prev.style,
        gradient: cloneGradient(prev.gradient),
        dynamicSeed: prev.dynamicSeed,
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
      const base = gradient
        ? cloneGradient(gradient)
        : cloneGradient(prev.gradient);
      const next = {
        ...prev,
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
        dynamicSeed: snap.dynamicSeed,
      });
    }
    snapshotRef.current = null;
    setEditing(false);
  }, []);

  return {
    colorStyle: prefs.style,
    gradient: prefs.gradient,
    dynamicSeed: prefs.dynamicSeed,
    setColorStyle,
    setGradient,
    reshuffleDynamic,
    beginGradientEdit,
    previewGradient,
    commitGradientEdit,
    cancelGradientEdit,
    editingGradient: editing,
  };
}
