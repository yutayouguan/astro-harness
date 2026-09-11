import type { ActiveUiStyle } from "./activeUiStyle.ts";
import type { PetScene } from "./petScene.ts";
import {
  addRecentWallpaper,
  type WallpaperPrefs,
  type WallpaperAsset,
} from "./wallpaper.ts";

/** Materialize the effective style without losing its crop, shading or color policy. */
export function materializeWallpaper(
  prefs: WallpaperPrefs,
  style: ActiveUiStyle | null,
  theme: "light" | "dark",
): WallpaperPrefs {
  if (!style?.wallpaper) return structuredClone(prefs);
  const w = style.wallpaper;
  const asset: WallpaperAsset = {
    id: style.id,
    name: style.name,
    path: w.path,
    source: "ai",
    width: 0,
    height: 0,
    createdAt: style.updatedAt,
    recommendedTheme: w.recommendedTheme,
    accentColor: w.accentColor,
    secondaryColor: w.secondaryColor,
  };
  const tokens = style.tokens[theme];
  return {
    ...addRecentWallpaper(prefs, asset),
    fit: w.fit,
    shade: w.shade,
    blur: w.blur,
    adaptiveColor: w.adaptiveColor,
    followSystemWallpaper: false,
    customThemeColor: tokens["--color-accent"] ?? prefs.customThemeColor,
    customHighlightColor:
      tokens["--color-accent-secondary"] ?? prefs.customHighlightColor,
  };
}

export function ambienceWallpaperChoices(
  recent: WallpaperAsset[],
  scenes: PetScene[],
) {
  const choices = new Map(recent.map((asset) => [asset.path, asset]));
  for (const scene of scenes) {
    if (scene.wallpaperPath && !choices.has(scene.wallpaperPath))
      choices.set(scene.wallpaperPath, {
        id: scene.id,
        name: scene.name,
        source: "ai",
        width: 0,
        height: 0,
        createdAt: scene.style?.updatedAt ?? "",
        ...scene.style?.wallpaper,
        path: scene.wallpaperPath,
      });
  }
  return [...choices.values()];
}

export function paletteTokens(primary: string, secondary: string) {
  if (![primary, secondary].every((color) => /^#[0-9a-f]{6}$/i.test(color)))
    throw new Error("请选择有效的六位颜色");
  const tokens = {
    "--color-accent": primary,
    "--color-accent-secondary": secondary,
  };
  return { light: { ...tokens }, dark: { ...tokens } };
}

export function explicitStylePalette(
  style: ActiveUiStyle | null,
  theme: "light" | "dark",
) {
  if (!style || style.wallpaper?.adaptiveColor !== false) return null;
  const tokens = style.tokens[theme];
  const themeColor = tokens["--color-accent"];
  const highlightColor = tokens["--color-accent-secondary"];
  return [themeColor, highlightColor].every(
    (color) => typeof color === "string" && /^#[0-9a-f]{6}$/i.test(color),
  )
    ? { themeColor, highlightColor }
    : null;
}

/** Analysis metadata may arrive later; it must not invalidate a user's undo. */
export function ambiencePreferenceKey(prefs: WallpaperPrefs) {
  return JSON.stringify([
    prefs.mode,
    prefs.current?.path ?? null,
    prefs.fit,
    prefs.shade,
    prefs.blur,
    prefs.adaptiveColor,
    prefs.customThemeColor,
    prefs.customHighlightColor,
    prefs.followSystemWallpaper,
  ]);
}
