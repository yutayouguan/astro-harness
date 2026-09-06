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
import {
  resolveThemePreference,
  type ResolvedTheme,
  type ThemeMode,
} from "../../lib/ui/themeResolution";
import {
  applyGlassIntensity,
  DEFAULT_GLASS_INTENSITY,
  normalizeGlassIntensity,
  persistGlassIntensity,
  readStoredGlassIntensity,
  type GlassIntensity,
} from "../../lib/ui/glassIntensity";

export type { ResolvedTheme, ThemeMode } from "../../lib/ui/themeResolution";
export type { GlassIntensity } from "../../lib/ui/glassIntensity";

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
async function syncNativeWindowTheme(
  mode: ThemeMode,
  resolved: ResolvedTheme,
  wallpaperTheme: ResolvedTheme | null,
) {
  try {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    // auto → null 跟随系统；light/dark 锁定，防止切 tab 时原生外观翻转
    await getCurrentWindow().setTheme(
      mode === "auto" && wallpaperTheme == null ? null : resolved,
    );
  } catch {
    // 浏览器预览或 API 不可用时忽略
  }
}

type ThemeContextValue = {
  mode: ThemeMode;
  setMode: (mode: ThemeMode) => void;
  resolved: ResolvedTheme;
  glassIntensity: GlassIntensity;
  setGlassIntensity: (intensity: GlassIntensity) => void;
  setWallpaperTheme: (theme: ResolvedTheme | null) => void;
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
  const [glassIntensity, setGlassIntensityState] = useState<GlassIntensity>(
    () =>
      typeof window === "undefined"
        ? DEFAULT_GLASS_INTENSITY
        : readStoredGlassIntensity(),
  );
  const [wallpaperTheme, setWallpaperThemeState] =
    useState<ResolvedTheme | null>(null);

  const apply = useCallback(
    (nextMode: ThemeMode, nextWallpaperTheme: ResolvedTheme | null) => {
      const next = resolveThemePreference(
        nextMode,
        systemPrefersDark(),
        nextWallpaperTheme,
      );
      setResolved(next);
      applyResolved(next);
      persistMode(nextMode);
      void syncNativeWindowTheme(nextMode, next, nextWallpaperTheme);
    },
    [],
  );

  useEffect(() => {
    apply(mode, wallpaperTheme);
    applyGlassIntensity(document.documentElement, glassIntensity);
  }, [mode, apply, glassIntensity, wallpaperTheme]);

  useEffect(() => {
    if (mode !== "auto") return;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => {
      apply("auto", wallpaperTheme);
    };
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, [mode, apply, wallpaperTheme]);

  const setMode = useCallback((next: ThemeMode) => {
    setModeState(next);
  }, []);

  const setGlassIntensity = useCallback((intensity: GlassIntensity) => {
    const normalized = normalizeGlassIntensity(intensity);
    setGlassIntensityState(normalized);
    applyGlassIntensity(document.documentElement, normalized);
    persistGlassIntensity(normalized);
  }, []);

  const setWallpaperTheme = useCallback((theme: ResolvedTheme | null) => {
    setWallpaperThemeState((current) => (current === theme ? current : theme));
  }, []);

  const reassert = useCallback(() => {
    applyResolved(
      resolveThemePreference(mode, systemPrefersDark(), wallpaperTheme),
    );
    applyGlassIntensity(document.documentElement, glassIntensity);
  }, [mode, glassIntensity, wallpaperTheme]);

  const value = useMemo(
    () => ({
      mode,
      setMode,
      resolved,
      glassIntensity,
      setGlassIntensity,
      setWallpaperTheme,
      reassert,
    }),
    [
      mode,
      setMode,
      resolved,
      glassIntensity,
      setGlassIntensity,
      setWallpaperTheme,
      reassert,
    ],
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
