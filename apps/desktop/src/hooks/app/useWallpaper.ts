import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  DEFAULT_WALLPAPER_PREFS,
  addRecentWallpaper,
  cycleRecentWallpaper,
  normalizeWallpaperPrefs,
  type WallpaperAsset,
  type WallpaperFit,
  type WallpaperMode,
  type WallpaperPrefs,
} from "../../lib/ui/wallpaper";

const STORAGE_KEY = "astro-wallpaper-prefs.v1";

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

export type WallpaperController = {
  prefs: WallpaperPrefs;
  busy: "upload" | "generate" | null;
  error: string | null;
  setMode: (mode: WallpaperMode) => void;
  setFit: (fit: WallpaperFit) => void;
  setShade: (shade: number) => void;
  setBlur: (blur: number) => void;
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

  const update = useCallback((recipe: (current: WallpaperPrefs) => WallpaperPrefs) => {
    setPrefs((current) => {
      const next = normalizeWallpaperPrefs(recipe(current));
      persist(next);
      return next;
    });
  }, []);

  const applyAsset = useCallback(
    (asset: WallpaperAsset) => {
      setError(null);
      update((current) => addRecentWallpaper(current, asset));
    },
    [update],
  );

  const setMode = useCallback(
    (mode: WallpaperMode) => {
      update((current) => ({ ...current, mode }));
    },
    [update],
  );
  const setFit = useCallback(
    (fit: WallpaperFit) => update((current) => ({ ...current, fit })),
    [update],
  );
  const setShade = useCallback(
    (shade: number) => update((current) => ({ ...current, shade })),
    [update],
  );
  const setBlur = useCallback(
    (blur: number) => update((current) => ({ ...current, blur })),
    [update],
  );
  const select = useCallback((asset: WallpaperAsset) => applyAsset(asset), [applyAsset]);
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
      return {
        ...current,
        mode: "color",
        current: null,
        recent: unavailableId
          ? current.recent.filter((asset) => asset.id !== unavailableId)
          : current.recent,
      };
    });
    setError("壁纸文件不可用，已恢复为氛围配色");
  }, [update]);

  useEffect(() => {
    const asset = prefs.current;
    if (!asset || asset.recommendedTheme || analysisRequests.current.has(asset.path)) {
      return;
    }
    analysisRequests.current.add(asset.path);
    let cancelled = false;
    void invoke<WallpaperAnalysis>("analyze_wallpaper", { path: asset.path })
      .then((analysis) => {
        if (cancelled) return;
        update((current) => {
          const enrich = (candidate: WallpaperAsset) =>
            candidate.id === asset.id ? { ...candidate, ...analysis } : candidate;
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
    select,
    cycleRecent,
    importImage,
    generate,
    clearError,
    markCurrentUnavailable,
  };
}
