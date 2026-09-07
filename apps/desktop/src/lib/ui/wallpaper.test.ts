import { test } from "node:test";
import assert from "node:assert/strict";
import {
  DEFAULT_WALLPAPER_PREFS,
  DEFAULT_WALLPAPER_HIGHLIGHT_COLOR,
  DEFAULT_WALLPAPER_THEME_COLOR,
  MAX_RECENT_WALLPAPERS,
  addRecentWallpaper,
  applySystemWallpaper,
  cycleRecentWallpaper,
  normalizeWallpaperPrefs,
  resolveExtractedWallpaperPalette,
  resolveWallpaperPalette,
  wallpaperBackgroundSize,
  type WallpaperAsset,
} from "./wallpaper.ts";

const asset = (id: string): WallpaperAsset => ({
  id,
  path: `/tmp/${id}.png`,
  name: `${id}.png`,
  source: "upload",
  width: 1200,
  height: 800,
  createdAt: "2026-09-05T00:00:00Z",
});

test("invalid stored wallpaper prefs use defaults and empty wallpaper mode stays selectable", () => {
  assert.deepEqual(normalizeWallpaperPrefs(null), DEFAULT_WALLPAPER_PREFS);
  assert.equal(
    normalizeWallpaperPrefs({ mode: "wallpaper", current: null }).mode,
    "wallpaper",
  );
});

test("new installs follow the system wallpaper while legacy custom prefs do not", () => {
  assert.equal(DEFAULT_WALLPAPER_PREFS.mode, "wallpaper");
  assert.equal(DEFAULT_WALLPAPER_PREFS.followSystemWallpaper, true);
  const legacyDefault = normalizeWallpaperPrefs({
    mode: "color",
    current: null,
    recent: [],
  });
  assert.equal(legacyDefault.mode, "wallpaper");
  assert.equal(legacyDefault.followSystemWallpaper, true);
  assert.equal(
    normalizeWallpaperPrefs({
      mode: "wallpaper",
      current: asset("legacy"),
    }).followSystemWallpaper,
    false,
  );
});

test("normalization clamps controls and rejects malformed assets", () => {
  const prefs = normalizeWallpaperPrefs({
    mode: "wallpaper",
    current: {
      ...asset("current"),
      luminance: 3,
      recommendedTheme: "dark",
      accentColor: "#22c55e",
      secondaryColor: "#3b82f6",
    },
    recent: [asset("current"), { id: "broken" }],
    fit: "stretch",
    shade: 900,
    blur: -2,
    adaptiveColor: false,
    customThemeColor: "#F97316",
    customHighlightColor: "not-a-color",
  });
  assert.equal(prefs.mode, "wallpaper");
  assert.equal(prefs.fit, "stretch");
  assert.equal(prefs.shade, 55);
  assert.equal(prefs.blur, 0);
  assert.equal(prefs.current?.luminance, 1);
  assert.equal(prefs.current?.recommendedTheme, "dark");
  assert.equal(prefs.current?.accentColor, "#22c55e");
  assert.equal(prefs.current?.secondaryColor, "#3b82f6");
  assert.equal(prefs.adaptiveColor, false);
  assert.equal(prefs.customThemeColor, "#f97316");
  assert.equal(prefs.customHighlightColor, undefined);
  assert.equal(prefs.followSystemWallpaper, false);
  assert.deepEqual(
    prefs.recent.map((item) => item.id),
    ["current"],
  );
});

test("wallpaper palette defaults to extracted colors and accepts a manual override", () => {
  const current = {
    ...asset("palette"),
    accentColor: "#22C55E",
    secondaryColor: "#3B82F6",
  };
  assert.deepEqual(
    resolveWallpaperPalette(DEFAULT_WALLPAPER_PREFS, current, "light"),
    {
      themeColor: "#1ca24d",
      highlightColor: "#0a58d7",
    },
  );
  assert.deepEqual(
    resolveWallpaperPalette(
      {
        adaptiveColor: false,
        customThemeColor: "#F97316",
        customHighlightColor: "#EC4899",
      },
      current,
      "dark",
    ),
    { themeColor: "#f97316", highlightColor: "#ec4899" },
  );
  assert.equal(
    resolveWallpaperPalette(
      {
        adaptiveColor: false,
        customThemeColor: DEFAULT_WALLPAPER_THEME_COLOR,
        customHighlightColor: undefined,
      },
      current,
      "light",
    ),
    null,
  );
  assert.match(DEFAULT_WALLPAPER_HIGHLIGHT_COLOR, /^#[0-9a-f]{6}$/);
});

function colorMetrics(color: string) {
  const channels = [1, 3, 5].map((offset) =>
    Number.parseInt(color.slice(offset, offset + 2), 16),
  );
  return {
    lightness: (Math.max(...channels) + Math.min(...channels)) / 510,
    luma:
      (0.2126 * channels[0] + 0.7152 * channels[1] + 0.0722 * channels[2]) /
      255,
  };
}

test("extracted wallpaper colors adapt to the active light and dark theme", () => {
  const wallpaper = {
    accentColor: "#f5d90a",
    secondaryColor: "#000020",
  };
  const light = resolveExtractedWallpaperPalette(wallpaper, "light");
  const dark = resolveExtractedWallpaperPalette(wallpaper, "dark");
  assert.ok(light);
  assert.ok(dark);
  assert.notDeepEqual(light, dark);

  for (const color of [light.themeColor, light.highlightColor]) {
    const metrics = colorMetrics(color);
    assert.ok(metrics.lightness <= 0.445, `${color} is too light`);
    assert.ok(metrics.luma <= 0.505, `${color} is too bright`);
  }
  for (const color of [dark.themeColor, dark.highlightColor]) {
    const metrics = colorMetrics(color);
    assert.ok(metrics.luma >= 0.215, `${color} is too dark`);
    assert.ok(metrics.luma <= 0.685, `${color} is too bright`);
  }
});

test("recent wallpapers are deduplicated and bounded", () => {
  let prefs = DEFAULT_WALLPAPER_PREFS;
  for (let index = 0; index < MAX_RECENT_WALLPAPERS + 2; index += 1) {
    prefs = addRecentWallpaper(prefs, asset(String(index)));
  }
  prefs = addRecentWallpaper(prefs, asset("4"));
  assert.equal(prefs.recent.length, MAX_RECENT_WALLPAPERS);
  assert.equal(prefs.recent[0].id, "4");
  assert.equal(
    new Set(prefs.recent.map((item) => item.id)).size,
    prefs.recent.length,
  );
  assert.equal(prefs.followSystemWallpaper, false);
});

test("system wallpaper becomes current without entering custom history", () => {
  const custom = asset("custom");
  const system: WallpaperAsset = {
    ...asset("system"),
    source: "system",
  };
  const prefs = applySystemWallpaper(
    { ...DEFAULT_WALLPAPER_PREFS, recent: [custom] },
    system,
  );
  assert.equal(prefs.current?.source, "system");
  assert.equal(prefs.followSystemWallpaper, true);
  assert.deepEqual(prefs.recent, [custom]);
});

test("stretch uses explicit dimensions while other fits pass through", () => {
  assert.equal(wallpaperBackgroundSize("stretch"), "100% 100%");
  assert.equal(wallpaperBackgroundSize("cover"), "cover");
  assert.equal(wallpaperBackgroundSize("contain"), "contain");
});

test("recent wallpaper cycle advances and wraps without changing a single item", () => {
  const first = asset("first");
  const second = asset("second");
  const prefs = {
    ...DEFAULT_WALLPAPER_PREFS,
    mode: "wallpaper" as const,
    current: first,
    recent: [first, second],
  };
  assert.equal(cycleRecentWallpaper(prefs).current?.id, "second");
  assert.equal(cycleRecentWallpaper(prefs).followSystemWallpaper, false);
  assert.equal(
    cycleRecentWallpaper({ ...prefs, current: second }).current?.id,
    "first",
  );
  assert.equal(
    cycleRecentWallpaper({ ...prefs, recent: [first] }).current?.id,
    "first",
  );
});
