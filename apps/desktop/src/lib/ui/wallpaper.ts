export type WallpaperMode = "color" | "wallpaper";
export type WallpaperFit = "cover" | "contain" | "stretch";

export type WallpaperAsset = {
  id: string;
  path: string;
  name: string;
  source: "upload" | "ai";
  width: number;
  height: number;
  createdAt: string;
  provider?: string;
  model?: string;
};

export type WallpaperPrefs = {
  mode: WallpaperMode;
  current: WallpaperAsset | null;
  recent: WallpaperAsset[];
  fit: WallpaperFit;
  shade: number;
  blur: number;
};

export const MAX_RECENT_WALLPAPERS = 6;

export const DEFAULT_WALLPAPER_PREFS: WallpaperPrefs = {
  mode: "color",
  current: null,
  recent: [],
  fit: "cover",
  shade: 34,
  blur: 0,
};

function clamp(value: unknown, min: number, max: number, fallback: number) {
  return typeof value === "number" && Number.isFinite(value)
    ? Math.min(max, Math.max(min, Math.round(value)))
    : fallback;
}

function normalizeAsset(raw: unknown): WallpaperAsset | null {
  if (!raw || typeof raw !== "object") return null;
  const value = raw as Record<string, unknown>;
  if (
    typeof value.id !== "string" ||
    !value.id.trim() ||
    typeof value.path !== "string" ||
    !value.path.trim() ||
    typeof value.name !== "string" ||
    (value.source !== "upload" && value.source !== "ai")
  ) {
    return null;
  }
  return {
    id: value.id,
    path: value.path,
    name: value.name,
    source: value.source,
    width: clamp(value.width, 0, 16_384, 0),
    height: clamp(value.height, 0, 16_384, 0),
    createdAt:
      typeof value.createdAt === "string" ? value.createdAt : "",
    provider:
      typeof value.provider === "string" ? value.provider : undefined,
    model: typeof value.model === "string" ? value.model : undefined,
  };
}

export function normalizeWallpaperPrefs(raw: unknown): WallpaperPrefs {
  if (!raw || typeof raw !== "object") return { ...DEFAULT_WALLPAPER_PREFS };
  const value = raw as Record<string, unknown>;
  const fit: WallpaperFit =
    value.fit === "contain" || value.fit === "stretch" ? value.fit : "cover";
  const recent = Array.isArray(value.recent)
    ? value.recent
        .map(normalizeAsset)
        .filter((asset): asset is WallpaperAsset => asset !== null)
        .filter(
          (asset, index, assets) =>
            assets.findIndex((candidate) => candidate.id === asset.id) === index,
        )
        .slice(0, MAX_RECENT_WALLPAPERS)
    : [];
  const current = normalizeAsset(value.current);
  const mode: WallpaperMode =
    value.mode === "wallpaper" ? "wallpaper" : "color";
  return {
    mode,
    current,
    recent,
    fit,
    shade: clamp(value.shade, 0, 55, DEFAULT_WALLPAPER_PREFS.shade),
    blur: clamp(value.blur, 0, 12, DEFAULT_WALLPAPER_PREFS.blur),
  };
}

export function addRecentWallpaper(
  prefs: WallpaperPrefs,
  asset: WallpaperAsset,
): WallpaperPrefs {
  return {
    ...prefs,
    mode: "wallpaper",
    current: asset,
    recent: [asset, ...prefs.recent.filter((item) => item.id !== asset.id)].slice(
      0,
      MAX_RECENT_WALLPAPERS,
    ),
  };
}

export function cycleRecentWallpaper(prefs: WallpaperPrefs): WallpaperPrefs {
  if (prefs.recent.length < 2) return prefs;
  const currentIndex = prefs.current
    ? prefs.recent.findIndex((asset) => asset.id === prefs.current?.id)
    : -1;
  const nextIndex = currentIndex >= 0 ? (currentIndex + 1) % prefs.recent.length : 0;
  return {
    ...prefs,
    mode: "wallpaper",
    current: prefs.recent[nextIndex],
  };
}

export function wallpaperBackgroundSize(fit: WallpaperFit): string {
  return fit === "stretch" ? "100% 100%" : fit;
}
