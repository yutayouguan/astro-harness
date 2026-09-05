export type ThemeMode = "light" | "dark" | "auto";
export type ResolvedTheme = "light" | "dark";

export function resolveThemePreference(
  mode: ThemeMode,
  systemDark: boolean,
  wallpaperTheme: ResolvedTheme | null,
): ResolvedTheme {
  if (mode !== "auto") return mode;
  return wallpaperTheme ?? (systemDark ? "dark" : "light");
}
