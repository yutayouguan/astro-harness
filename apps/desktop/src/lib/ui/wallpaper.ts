export type WallpaperMode = "color" | "wallpaper";
export type WallpaperFit = "cover" | "contain" | "stretch";

export type WallpaperAsset = {
  id: string;
  path: string;
  name: string;
  source: "system" | "upload" | "ai";
  width: number;
  height: number;
  createdAt: string;
  luminance?: number;
  recommendedTheme?: "light" | "dark";
  accentColor?: string;
  secondaryColor?: string;
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
  adaptiveColor: boolean;
  followSystemWallpaper: boolean;
};

export const MAX_RECENT_WALLPAPERS = 6;

export const DEFAULT_WALLPAPER_PREFS: WallpaperPrefs = {
  mode: "wallpaper",
  current: null,
  recent: [],
  fit: "cover",
  shade: 34,
  blur: 0,
  adaptiveColor: true,
  followSystemWallpaper: true,
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
    (value.source !== "system" &&
      value.source !== "upload" &&
      value.source !== "ai")
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
    createdAt: typeof value.createdAt === "string" ? value.createdAt : "",
    luminance:
      typeof value.luminance === "number" && Number.isFinite(value.luminance)
        ? Math.min(1, Math.max(0, value.luminance))
        : undefined,
    recommendedTheme:
      value.recommendedTheme === "dark" || value.recommendedTheme === "light"
        ? value.recommendedTheme
        : undefined,
    accentColor:
      typeof value.accentColor === "string" &&
      /^#[0-9a-f]{6}$/i.test(value.accentColor)
        ? value.accentColor
        : undefined,
    secondaryColor:
      typeof value.secondaryColor === "string" &&
      /^#[0-9a-f]{6}$/i.test(value.secondaryColor)
        ? value.secondaryColor
        : undefined,
    provider: typeof value.provider === "string" ? value.provider : undefined,
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
            assets.findIndex((candidate) => candidate.id === asset.id) ===
            index,
        )
        .slice(0, MAX_RECENT_WALLPAPERS)
    : [];
  const current = normalizeAsset(value.current);
  const legacySystemDefault =
    typeof value.followSystemWallpaper !== "boolean" &&
    !current &&
    recent.length === 0;
  const mode: WallpaperMode =
    value.mode === "wallpaper" || legacySystemDefault ? "wallpaper" : "color";
  return {
    mode,
    current,
    recent,
    fit,
    shade: clamp(value.shade, 0, 55, DEFAULT_WALLPAPER_PREFS.shade),
    blur: clamp(value.blur, 0, 12, DEFAULT_WALLPAPER_PREFS.blur),
    adaptiveColor:
      typeof value.adaptiveColor === "boolean"
        ? value.adaptiveColor
        : DEFAULT_WALLPAPER_PREFS.adaptiveColor,
    followSystemWallpaper:
      typeof value.followSystemWallpaper === "boolean"
        ? value.followSystemWallpaper
        : current?.source === "system" || legacySystemDefault,
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
    followSystemWallpaper: false,
    recent: [
      asset,
      ...prefs.recent.filter((item) => item.id !== asset.id),
    ].slice(0, MAX_RECENT_WALLPAPERS),
  };
}

export function applySystemWallpaper(
  prefs: WallpaperPrefs,
  asset: WallpaperAsset,
): WallpaperPrefs {
  return {
    ...prefs,
    mode: "wallpaper",
    current: asset,
    followSystemWallpaper: true,
    recent: prefs.recent.filter((item) => item.source !== "system"),
  };
}

export function cycleRecentWallpaper(prefs: WallpaperPrefs): WallpaperPrefs {
  if (prefs.recent.length < 2) return prefs;
  const currentIndex = prefs.current
    ? prefs.recent.findIndex((asset) => asset.id === prefs.current?.id)
    : -1;
  const nextIndex =
    currentIndex >= 0 ? (currentIndex + 1) % prefs.recent.length : 0;
  return {
    ...prefs,
    mode: "wallpaper",
    current: prefs.recent[nextIndex],
    followSystemWallpaper: false,
  };
}

export function wallpaperBackgroundSize(fit: WallpaperFit): string {
  return fit === "stretch" ? "100% 100%" : fit;
}

const WALLPAPER_PALETTE_PROPERTIES = [
  "--wallpaper-tone",
  "--wallpaper-tone-soft",
  "--wallpaper-tone-glow",
  "--wallpaper-accent-2",
] as const;

export function applyWallpaperPaletteVars(
  element: HTMLElement,
  accentColor: string,
  secondaryColor: string,
): void {
  element.style.setProperty("--wallpaper-tone", accentColor);
  element.style.setProperty(
    "--wallpaper-tone-soft",
    `color-mix(in srgb, ${accentColor} 28%, transparent)`,
  );
  element.style.setProperty(
    "--wallpaper-tone-glow",
    `color-mix(in srgb, ${accentColor} 40%, transparent)`,
  );
  element.style.setProperty("--wallpaper-accent-2", secondaryColor);
}

export function clearWallpaperPaletteVars(element: HTMLElement): void {
  for (const property of WALLPAPER_PALETTE_PROPERTIES) {
    element.style.removeProperty(property);
  }
}
