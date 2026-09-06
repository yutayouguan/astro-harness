import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  DEFAULT_WALLPAPER_PREFS,
  DEFAULT_WALLPAPER_HIGHLIGHT_COLOR,
  DEFAULT_WALLPAPER_THEME_COLOR,
  addRecentWallpaper,
  applySystemWallpaper,
  cycleRecentWallpaper,
  normalizeWallpaperPrefs,
  type WallpaperAsset,
  type WallpaperFit,
  type WallpaperMode,
  type WallpaperPrefs,
} from "../../lib/ui/wallpaper";
import { UI_STYLE_RESET_EVENT } from "./useActiveUiStyle";

const STORAGE_KEY = "astro-wallpaper-prefs.v1";
const SYSTEM_WALLPAPER_POLL_MS = 15_000;

function readStored(): WallpaperPrefs {
  try {
    const value = localStorage.getItem(STORAGE_KEY);
    return value
      ? normalizeWallpaperPrefs(JSON.parse(value) as unknown)
      : { ...DEFAULT_WALLPAPER_PREFS };
  } catch {
    return { ...DEFAULT_WALLPAPER_PREFS };
  }
}

function persist(prefs: WallpaperPrefs) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(prefs));
  } catch {
    // WebView 存储不可用时仍保留本次运行状态。
  }
}

function deactivateGeneratedStyle() {
  if (typeof window === "undefined") return;
  window.dispatchEvent(new Event(UI_STYLE_RESET_EVENT));
  void invoke("reset_active_ui_style").catch(() => {
    // 文件态样式不可用时，仍允许修改当前 WebView 的手动壁纸。
  });
}

export type WallpaperController = {
  prefs: WallpaperPrefs;
  busy: "upload" | "generate" | null;
  error: string | null;
  setMode: (mode: WallpaperMode) => void;
  setFit: (fit: WallpaperFit) => void;
  setShade: (shade: number) => void;
  setBlur: (blur: number) => void;
  setAdaptiveColor: (enabled: boolean) => void;
  setPalette: (themeColor: string, highlightColor: string) => void;
  setFollowSystemWallpaper: (enabled: boolean) => void;
  select: (asset: WallpaperAsset) => void;
  cycleRecent: () => void;
  importImage: (sourcePath: string) => Promise<WallpaperAsset>;
  generate: (prompt: string) => Promise<WallpaperAsset>;
  clearError: () => void;
  markCurrentUnavailable: () => void;
};

type WallpaperAnalysis = {
  luminance: number;
  recommendedTheme: "light" | "dark";
  accentColor: string;
  secondaryColor: string;
};

export function useWallpaper(): WallpaperController {
  const [prefs, setPrefs] = useState<WallpaperPrefs>(() =>
    typeof window === "undefined"
      ? { ...DEFAULT_WALLPAPER_PREFS }
      : readStored(),
  );
  const [busy, setBusy] = useState<WallpaperController["busy"]>(null);
  const [error, setError] = useState<string | null>(null);
  const analysisRequests = useRef(new Set<string>());
  const systemSyncInFlight = useRef(false);

  const update = useCallback(
    (recipe: (current: WallpaperPrefs) => WallpaperPrefs) => {
      setPrefs((current) => {
        const candidate = recipe(current);
        if (candidate === current) return current;
        const next = normalizeWallpaperPrefs(candidate);
        persist(next);
        return next;
      });
    },
    [],
  );

  const applyAsset = useCallback(
    (asset: WallpaperAsset) => {
      setError(null);
      update((current) => addRecentWallpaper(current, asset));
    },
    [update],
  );

  const setMode = useCallback(
    (mode: WallpaperMode) => {
      deactivateGeneratedStyle();
      update((current) => ({ ...current, mode }));
    },
    [update],
  );
  const setFit = useCallback(
    (fit: WallpaperFit) => {
      deactivateGeneratedStyle();
      update((current) => ({ ...current, fit }));
    },
    [update],
  );
  const setShade = useCallback(
    (shade: number) => {
      deactivateGeneratedStyle();
      update((current) => ({ ...current, shade }));
    },
    [update],
  );
  const setBlur = useCallback(
    (blur: number) => {
      deactivateGeneratedStyle();
      update((current) => ({ ...current, blur }));
    },
    [update],
  );
  const setAdaptiveColor = useCallback(
    (adaptiveColor: boolean) => {
      deactivateGeneratedStyle();
      update((current) => ({
        ...current,
        adaptiveColor,
        customThemeColor:
          current.customThemeColor ??
          current.current?.accentColor ??
          DEFAULT_WALLPAPER_THEME_COLOR,
        customHighlightColor:
          current.customHighlightColor ??
          current.current?.secondaryColor ??
          DEFAULT_WALLPAPER_HIGHLIGHT_COLOR,
      }));
    },
    [update],
  );
  const setPalette = useCallback(
    (customThemeColor: string, customHighlightColor: string) => {
      deactivateGeneratedStyle();
      update((current) => ({
        ...current,
        adaptiveColor: false,
        customThemeColor,
        customHighlightColor,
      }));
    },
    [update],
  );
  const setFollowSystemWallpaper = useCallback(
    (followSystemWallpaper: boolean) => {
      deactivateGeneratedStyle();
      update((current) => ({
        ...current,
        mode: followSystemWallpaper ? "wallpaper" : current.mode,
        followSystemWallpaper,
      }));
    },
    [update],
  );
  const select = useCallback(
    (asset: WallpaperAsset) => {
      deactivateGeneratedStyle();
      applyAsset(asset);
    },
    [applyAsset],
  );
  const cycleRecent = useCallback(() => {
    setError(null);
    update(cycleRecentWallpaper);
  }, [update]);

  const importImage = useCallback(
    async (sourcePath: string) => {
      setBusy("upload");
      setError(null);
      try {
        const asset = await invoke<WallpaperAsset>("import_wallpaper", {
          sourcePath,
        });
        deactivateGeneratedStyle();
        applyAsset(asset);
        return asset;
      } catch (cause) {
        const message = cause instanceof Error ? cause.message : String(cause);
        setError(message);
        throw cause;
      } finally {
        setBusy(null);
      }
    },
    [applyAsset],
  );

  const generate = useCallback(
    async (prompt: string) => {
      setBusy("generate");
      setError(null);
      try {
        const asset = await invoke<WallpaperAsset>("generate_wallpaper", {
          prompt,
        });
        deactivateGeneratedStyle();
        applyAsset(asset);
        return asset;
      } catch (cause) {
        const message = cause instanceof Error ? cause.message : String(cause);
        setError(message);
        throw cause;
      } finally {
        setBusy(null);
      }
    },
    [applyAsset],
  );

  const clearError = useCallback(() => setError(null), []);
  const markCurrentUnavailable = useCallback(() => {
    update((current) => {
      const unavailableId = current.current?.id;
      const isSystemWallpaper = current.current?.source === "system";
      return {
        ...current,
        mode: isSystemWallpaper ? "wallpaper" : "color",
        current: null,
        recent: unavailableId
          ? current.recent.filter((asset) => asset.id !== unavailableId)
          : current.recent,
      };
    });
    setError("壁纸文件不可用");
  }, [update]);

  const syncSystemWallpaper = useCallback(async () => {
    if (systemSyncInFlight.current) return;
    systemSyncInFlight.current = true;
    try {
      const asset = await invoke<WallpaperAsset>("get_system_wallpaper");
      setError(null);
      update((current) =>
        current.followSystemWallpaper && current.current?.id !== asset.id
          ? applySystemWallpaper(current, asset)
          : current,
      );
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : String(cause);
      setError(message);
    } finally {
      systemSyncInFlight.current = false;
    }
  }, [update]);

  useEffect(() => {
    if (
      prefs.mode !== "wallpaper" ||
      !prefs.followSystemWallpaper ||
      typeof window === "undefined"
    ) {
      return;
    }
    const refresh = () => {
      if (document.visibilityState === "visible") {
        void syncSystemWallpaper();
      }
    };
    void syncSystemWallpaper();
    const timer = window.setInterval(refresh, SYSTEM_WALLPAPER_POLL_MS);
    window.addEventListener("focus", refresh);
    document.addEventListener("visibilitychange", refresh);
    return () => {
      window.clearInterval(timer);
      window.removeEventListener("focus", refresh);
      document.removeEventListener("visibilitychange", refresh);
    };
  }, [prefs.followSystemWallpaper, prefs.mode, syncSystemWallpaper]);

  useEffect(() => {
    const asset = prefs.current;
    if (
      !asset ||
      (asset.recommendedTheme && asset.accentColor && asset.secondaryColor) ||
      analysisRequests.current.has(asset.path)
    ) {
      return;
    }
    analysisRequests.current.add(asset.path);
    let cancelled = false;
    void invoke<WallpaperAnalysis>("analyze_wallpaper", { path: asset.path })
      .then((analysis) => {
        if (cancelled) return;
        update((current) => {
          const enrich = (candidate: WallpaperAsset) =>
            candidate.id === asset.id
              ? { ...candidate, ...analysis }
              : candidate;
          return {
            ...current,
            current: current.current ? enrich(current.current) : null,
            recent: current.recent.map(enrich),
          };
        });
      })
      .catch(() => {
        // 旧壁纸分析失败时沿用系统主题，不影响壁纸显示。
      });
    return () => {
      cancelled = true;
      analysisRequests.current.delete(asset.path);
    };
  }, [
    prefs.current?.id,
    prefs.current?.path,
    prefs.current?.recommendedTheme,
    prefs.current?.accentColor,
    prefs.current?.secondaryColor,
    update,
  ]);

  return {
    prefs,
    busy,
    error,
    setMode,
    setFit,
    setShade,
    setBlur,
    setAdaptiveColor,
    setPalette,
    setFollowSystemWallpaper,
    select,
    cycleRecent,
    importImage,
    generate,
    clearError,
    markCurrentUnavailable,
  };
}
