import type { WallpaperPrefs } from "./wallpaper";

export type ActiveUiStyle = {
  schemaVersion: 1;
  id: string;
  name: string;
  revision: string;
  updatedAt: string;
  tokens: {
    light: Record<string, string>;
    dark: Record<string, string>;
  };
  icons: {
    motion?: "smooth" | "snappy" | "bouncy";
    strokeWidth?: 1 | 2 | 2.5;
  };
  wallpaper?: {
    path: string;
    fit: "cover" | "contain" | "stretch";
    shade: number;
    blur: number;
    adaptiveColor: boolean;
    recommendedTheme?: "light" | "dark";
    accentColor?: string;
    secondaryColor?: string;
  };
};

export function tokensForResolvedTheme(
  style: ActiveUiStyle | null,
  resolvedTheme: "light" | "dark",
): Record<string, string> {
  return style?.tokens?.[resolvedTheme] ?? {};
}

export function resolveWallpaperPresentation(
  style: ActiveUiStyle | null,
  prefs: WallpaperPrefs,
) {
  if (style?.wallpaper) {
    return {
      wallpaper: style.wallpaper,
      generated: true,
      fit: style.wallpaper.fit,
      shade: style.wallpaper.shade,
      blur: style.wallpaper.blur,
      adaptiveColor: style.wallpaper.adaptiveColor,
    } as const;
  }
  const wallpaper = prefs.mode === "wallpaper" ? prefs.current : null;
  return {
    wallpaper,
    generated: false,
    fit: prefs.fit,
    shade: prefs.shade,
    blur: prefs.blur,
    adaptiveColor: prefs.adaptiveColor,
  } as const;
}
