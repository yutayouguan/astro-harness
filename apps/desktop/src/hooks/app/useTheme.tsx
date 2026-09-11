/** 外观偏好：同步主题、材质强度与全局界面缩放。 */
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
import {
  applyInterfaceScale,
  DEFAULT_INTERFACE_SCALE,
  normalizeInterfaceScale,
  persistInterfaceScale,
  readStoredInterfaceScale,
  type InterfaceScale,
} from "../../lib/ui/interfaceScale";
import {
  readPreference,
  writePreference,
} from "../../lib/storage/preferenceStore";
import {
  applyInterfaceMaterial,
  normalizeInterfaceMaterial,
  persistInterfaceMaterial,
  readStoredInterfaceMaterial,
  type InterfaceMaterial,
} from "../../lib/ui/interfaceMaterial";

export type { ResolvedTheme, ThemeMode } from "../../lib/ui/themeResolution";
export type { GlassIntensity } from "../../lib/ui/glassIntensity";
export type { InterfaceScale } from "../../lib/ui/interfaceScale";

const STORAGE_KEY = "astro-theme-mode";

function readStoredMode(): ThemeMode {
  return readPreference(STORAGE_KEY, "auto", (value) =>
    value === "light" || value === "dark" || value === "auto" ? value : "auto",
  );
}

function persistMode(mode: ThemeMode) {
  writePreference(STORAGE_KEY, mode);
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
  material: InterfaceMaterial;
  setMaterial: (material: InterfaceMaterial) => void;
  mode: ThemeMode;
  setMode: (mode: ThemeMode) => void;
  resolved: ResolvedTheme;
  glassIntensity: GlassIntensity;
  setGlassIntensity: (intensity: GlassIntensity) => void;
  interfaceScale: InterfaceScale;
  setInterfaceScale: (scale: InterfaceScale) => void;
  setWallpaperTheme: (theme: ResolvedTheme | null) => void;
  /** 在切 tab / tone 后重新断言当前主题，防止 data-theme 被冲掉 */
  reassert: () => void;
};

const ThemeContext = createContext<ThemeContextValue | null>(null);

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [material, setMaterialState] = useState(readStoredInterfaceMaterial);
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
  const [interfaceScale, setInterfaceScaleState] = useState<InterfaceScale>(
    () =>
      typeof window === "undefined"
        ? DEFAULT_INTERFACE_SCALE
        : readStoredInterfaceScale(),
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
    applyInterfaceMaterial(document.documentElement, material);
    applyGlassIntensity(document.documentElement, glassIntensity);
    applyInterfaceScale(document.documentElement, interfaceScale);
  }, [mode, apply, material, glassIntensity, interfaceScale, wallpaperTheme]);

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

  const setMaterial = useCallback((next: InterfaceMaterial) => {
    const normalized = normalizeInterfaceMaterial(next);
    setMaterialState(normalized);
    applyInterfaceMaterial(document.documentElement, normalized);
    persistInterfaceMaterial(normalized);
  }, []);

  const setGlassIntensity = useCallback((intensity: GlassIntensity) => {
    const normalized = normalizeGlassIntensity(intensity);
    setGlassIntensityState(normalized);
    applyGlassIntensity(document.documentElement, normalized);
    persistGlassIntensity(normalized);
  }, []);

  const setInterfaceScale = useCallback((scale: InterfaceScale) => {
    const normalized = normalizeInterfaceScale(scale);
    setInterfaceScaleState(normalized);
    applyInterfaceScale(document.documentElement, normalized);
    persistInterfaceScale(normalized);
  }, []);

  const setWallpaperTheme = useCallback((theme: ResolvedTheme | null) => {
    setWallpaperThemeState((current) => (current === theme ? current : theme));
  }, []);

  const reassert = useCallback(() => {
    applyInterfaceMaterial(document.documentElement, material);
    applyResolved(
      resolveThemePreference(mode, systemPrefersDark(), wallpaperTheme),
    );
    applyGlassIntensity(document.documentElement, glassIntensity);
    applyInterfaceScale(document.documentElement, interfaceScale);
  }, [mode, material, glassIntensity, interfaceScale, wallpaperTheme]);

  const value = useMemo(
    () => ({
      material,
      setMaterial,
      mode,
      setMode,
      resolved,
      glassIntensity,
      setGlassIntensity,
      interfaceScale,
      setInterfaceScale,
      setWallpaperTheme,
      reassert,
    }),
    [
      material,
      setMaterial,
      mode,
      setMode,
      resolved,
      glassIntensity,
      setGlassIntensity,
      interfaceScale,
      setInterfaceScale,
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
