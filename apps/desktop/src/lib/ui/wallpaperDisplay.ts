import { normalizeWallpaperPrefs, type WallpaperPrefs } from "./wallpaper.ts";

export type WallpaperDisplay = Pick<WallpaperPrefs, "fit" | "shade" | "blur">;

/** Use exactly the same fit/shade/blur normalization as Appearance settings. */
export function wallpaperDisplayValue(
  value: Partial<WallpaperDisplay>,
): WallpaperDisplay {
  const { fit, shade, blur } = normalizeWallpaperPrefs(value);
  return { fit, shade, blur };
}
