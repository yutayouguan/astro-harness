/** 亮/暗/跟随系统主题：data-theme 与原生窗主题同步。 */
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";

export type ThemeMode = "light" | "dark" | "auto";
export type ResolvedTheme = "light" | "dark";
export type GlassLevel =
  "liquid" | "liquid-soft" | "rich" | "normal" | "minimal";

const GLASS_LEVELS: readonly GlassLevel[] = [
  "liquid",
  "liquid-soft",
  "rich",
  "normal",
  "minimal",
];
const DEFAULT_GLASS_LEVEL: GlassLevel = "liquid";

const STORAGE_KEY = "astro-theme-mode";
const GLASS_KEY = "astro-glass-level";

function readStoredMode(): ThemeMode {
  try {
    const v = localStorage.getItem(STORAGE_KEY);
    if (v === "light" || v === "dark" || v === "auto") return v;
  } catch {
    // ignore
  }
  return "auto";
}

function persistMode(mode: ThemeMode) {
  try {
    localStorage.setItem(STORAGE_KEY, mode);
  } catch {
    // ignore
  }
}

function systemPrefersDark(): boolean {
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

export function resolveTheme(mode: ThemeMode): ResolvedTheme {
  if (mode === "auto") return systemPrefersDark() ? "dark" : "light";
  return mode;
}

function readGlassLevel(): GlassLevel {
  try {
    const v = localStorage.getItem(GLASS_KEY);
    if (GLASS_LEVELS.includes(v as GlassLevel)) return v as GlassLevel;
  } catch {
    /* ignore */
  }
  return DEFAULT_GLASS_LEVEL;
}

function applyGlass(level: GlassLevel) {
  const root = document.documentElement;
  // rich 是 token 基线本身，不需要属性钩子。
  if (level === "rich") {
    root.removeAttribute("data-glass");
  } else {
    root.setAttribute("data-glass", level);
  }
}

function applyResolved(next: ResolvedTheme) {
  const root = document.documentElement;
  const prev = root.getAttribute("data-theme");
  root.dataset.theme = next;
  root.style.colorScheme = next;
  root.setAttribute("data-theme", next);

  if (
    prev &&
    prev !== next &&
    !window.matchMedia("(prefers-reduced-motion: reduce)").matches
  ) {
    root.classList.add("theme-transitioning");
    const tid = setTimeout(
      () => root.classList.remove("theme-transitioning"),
      350,
    );
    root.dataset.themeTimer = String(tid);
  }
}

/** 同步 Tauri 原生窗主题，避免 WKWebView 在换 underlay 时跟着系统外观跳变 */
async function syncNativeWindowTheme(mode: ThemeMode, resolved: ResolvedTheme) {
  try {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    // auto → null 跟随系统；light/dark 锁定，防止切 tab 时原生外观翻转
    await getCurrentWindow().setTheme(mode === "auto" ? null : resolved);
  } catch {
    // 浏览器预览或 API 不可用时忽略
  }
}

type ThemeContextValue = {
  mode: ThemeMode;
  setMode: (mode: ThemeMode) => void;
  resolved: ResolvedTheme;
  glassLevel: GlassLevel;
  setGlassLevel: (level: GlassLevel) => void;
  /** 在切 tab / tone 后重新断言当前主题，防止 data-theme 被冲掉 */
  reassert: () => void;
};

const ThemeContext = createContext<ThemeContextValue | null>(null);

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [mode, setModeState] = useState<ThemeMode>(() =>
    typeof window === "undefined" ? "auto" : readStoredMode(),
  );
  const [resolved, setResolved] = useState<ResolvedTheme>(() =>
    typeof window === "undefined" ? "dark" : resolveTheme(readStoredMode()),
  );
  const [glassLevel, setGlassState] = useState<GlassLevel>(() =>
    typeof window === "undefined" ? DEFAULT_GLASS_LEVEL : readGlassLevel(),
  );

  const apply = useCallback((nextMode: ThemeMode) => {
    const next = resolveTheme(nextMode);
    setResolved(next);
    applyResolved(next);
    persistMode(nextMode);
    void syncNativeWindowTheme(nextMode, next);
  }, []);

  useEffect(() => {
    apply(mode);
    applyGlass(glassLevel);
  }, [mode, apply, glassLevel]);

  useEffect(() => {
    if (mode !== "auto") return;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => {
      apply("auto");
    };
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, [mode, apply]);

  const setMode = useCallback((next: ThemeMode) => {
    setModeState(next);
  }, []);

  const setGlassLevel = useCallback((level: GlassLevel) => {
    setGlassState(level);
    applyGlass(level);
    try {
      localStorage.setItem(GLASS_KEY, level);
    } catch {
      /* ignore */
    }
  }, []);

  const reassert = useCallback(() => {
    applyResolved(resolveTheme(mode));
    applyGlass(glassLevel);
  }, [mode, glassLevel]);

  const value = useMemo(
    () => ({ mode, setMode, resolved, glassLevel, setGlassLevel, reassert }),
    [mode, setMode, resolved, glassLevel, setGlassLevel, reassert],
  );

  return (
    <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>
  );
}

export function useTheme(): ThemeContextValue {
  const ctx = useContext(ThemeContext);
  if (!ctx) {
    throw new Error("useTheme must be used within ThemeProvider");
  }
  return ctx;
}
