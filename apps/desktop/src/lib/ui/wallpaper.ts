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
  customThemeColor?: string;
  customHighlightColor?: string;
  followSystemWallpaper: boolean;
};

export type WallpaperPalette = {
  themeColor: string;
  highlightColor: string;
};

type ExtractedWallpaperColors = Pick<
  WallpaperAsset,
  "accentColor" | "secondaryColor"
>;

type WallpaperPaletteTheme = "light" | "dark";

export const MAX_RECENT_WALLPAPERS = 6;
export const DEFAULT_WALLPAPER_THEME_COLOR = "#4f6ef7";
export const DEFAULT_WALLPAPER_HIGHLIGHT_COLOR = "#22b8a7";
const EXTRACTED_COLOR_BOUNDS = {
  light: { minLightness: 0, maxLightness: 0.44, minLuma: 0, maxLuma: 0.5 },
  dark: {
    minLightness: 0.52,
    maxLightness: 0.58,
    minLuma: 0.22,
    maxLuma: 0.68,
  },
} as const;

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

function normalizeHexColor(value: unknown): string | undefined {
  return typeof value === "string" && /^#[0-9a-f]{6}$/i.test(value)
    ? value.toLowerCase()
    : undefined;
}

function rgbToHsl([red, green, blue]: number[]): [number, number, number] {
  const r = red / 255;
  const g = green / 255;
  const b = blue / 255;
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const delta = max - min;
  const lightness = (max + min) / 2;
  if (delta === 0) return [0, 0, lightness];

  const saturation = delta / (1 - Math.abs(2 * lightness - 1));
  const hue =
    max === r
      ? 60 * (((g - b) / delta) % 6)
      : max === g
        ? 60 * ((b - r) / delta + 2)
        : 60 * ((r - g) / delta + 4);
  return [hue < 0 ? hue + 360 : hue, saturation, lightness];
}

function hslToRgb(
  hue: number,
  saturation: number,
  lightness: number,
): number[] {
  const chroma = (1 - Math.abs(2 * lightness - 1)) * saturation;
  const section = (((hue % 360) + 360) % 360) / 60;
  const x = chroma * (1 - Math.abs((section % 2) - 1));
  const [red, green, blue] =
    section < 1
      ? [chroma, x, 0]
      : section < 2
        ? [x, chroma, 0]
        : section < 3
          ? [0, chroma, x]
          : section < 4
            ? [0, x, chroma]
            : section < 5
              ? [x, 0, chroma]
              : [chroma, 0, x];
  const offset = lightness - chroma / 2;
  return [red, green, blue].map((channel) =>
    Math.round((channel + offset) * 255),
  );
}

function adaptExtractedColor(
  value: unknown,
  theme: WallpaperPaletteTheme,
): string | undefined {
  const color = normalizeHexColor(value);
  if (!color) return undefined;

  const source = [1, 3, 5].map((offset) =>
    Number.parseInt(color.slice(offset, offset + 2), 16),
  );
  const [hue, saturation, sourceLightness] = rgbToHsl(source);
  const bounds = EXTRACTED_COLOR_BOUNDS[theme];
  const lightness = Math.min(
    bounds.maxLightness,
    Math.max(bounds.minLightness, sourceLightness),
  );
  let channels = hslToRgb(hue, saturation, lightness);
  const luma = () =>
    (0.2126 * channels[0] + 0.7152 * channels[1] + 0.0722 * channels[2]) / 255;
  const currentLuma = luma();
  if (currentLuma > bounds.maxLuma) {
    const scale = bounds.maxLuma / currentLuma;
    channels = channels.map((channel) => Math.round(channel * scale));
  } else if (currentLuma < bounds.minLuma) {
    const mix = (bounds.minLuma - currentLuma) / (1 - currentLuma);
    channels = channels.map((channel) =>
      Math.round(channel + (255 - channel) * mix),
    );
  }

  return `#${channels
    .map((channel) => channel.toString(16).padStart(2, "0"))
    .join("")}`;
}

export function resolveExtractedWallpaperPalette(
  wallpaper: ExtractedWallpaperColors | null,
  theme: WallpaperPaletteTheme,
): WallpaperPalette | null {
  const themeColor = adaptExtractedColor(wallpaper?.accentColor, theme);
  const highlightColor = adaptExtractedColor(wallpaper?.secondaryColor, theme);
  return themeColor && highlightColor ? { themeColor, highlightColor } : null;
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
    accentColor: normalizeHexColor(value.accentColor),
    secondaryColor: normalizeHexColor(value.secondaryColor),
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
    customThemeColor: normalizeHexColor(value.customThemeColor),
    customHighlightColor: normalizeHexColor(value.customHighlightColor),
    followSystemWallpaper:
      typeof value.followSystemWallpaper === "boolean"
        ? value.followSystemWallpaper
        : current?.source === "system" || legacySystemDefault,
  };
}

export function resolveWallpaperPalette(
  prefs: Pick<
    WallpaperPrefs,
    "adaptiveColor" | "customThemeColor" | "customHighlightColor"
  >,
  wallpaper: ExtractedWallpaperColors | null,
  theme: WallpaperPaletteTheme,
): WallpaperPalette | null {
  if (prefs.adaptiveColor)
    return resolveExtractedWallpaperPalette(wallpaper, theme);

  const themeColor = normalizeHexColor(prefs.customThemeColor);
  const highlightColor = normalizeHexColor(prefs.customHighlightColor);
  return themeColor && highlightColor ? { themeColor, highlightColor } : null;
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
