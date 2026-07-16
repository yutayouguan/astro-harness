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

const STORAGE_KEY = "astro-theme-mode";

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

function applyResolved(next: ResolvedTheme) {
  const root = document.documentElement;
  root.dataset.theme = next;
  root.style.colorScheme = next;
  // 显式锁定，避免被其它脚本/扩展改掉 data-theme
  root.setAttribute("data-theme", next);
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

  const apply = useCallback((nextMode: ThemeMode) => {
    const next = resolveTheme(nextMode);
    setResolved(next);
    applyResolved(next);
    persistMode(nextMode);
    void syncNativeWindowTheme(nextMode, next);
  }, []);

  useEffect(() => {
    apply(mode);
  }, [mode, apply]);

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

  const reassert = useCallback(() => {
    applyResolved(resolveTheme(mode));
  }, [mode]);

  const value = useMemo(
    () => ({ mode, setMode, resolved, reassert }),
    [mode, setMode, resolved, reassert],
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
