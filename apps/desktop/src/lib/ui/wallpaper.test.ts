import { test } from "node:test";
import assert from "node:assert/strict";
import {
  DEFAULT_WALLPAPER_PREFS,
  MAX_RECENT_WALLPAPERS,
  addRecentWallpaper,
  cycleRecentWallpaper,
  normalizeWallpaperPrefs,
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

test("normalization clamps controls and rejects malformed assets", () => {
  const prefs = normalizeWallpaperPrefs({
    mode: "wallpaper",
    current: {
      ...asset("current"),
      luminance: 3,
      recommendedTheme: "dark",
    },
    recent: [asset("current"), { id: "broken" }],
    fit: "stretch",
    shade: 900,
    blur: -2,
  });
  assert.equal(prefs.mode, "wallpaper");
  assert.equal(prefs.fit, "stretch");
  assert.equal(prefs.shade, 55);
  assert.equal(prefs.blur, 0);
  assert.equal(prefs.current?.luminance, 1);
  assert.equal(prefs.current?.recommendedTheme, "dark");
  assert.deepEqual(prefs.recent.map((item) => item.id), ["current"]);
});

test("recent wallpapers are deduplicated and bounded", () => {
  let prefs = DEFAULT_WALLPAPER_PREFS;
  for (let index = 0; index < MAX_RECENT_WALLPAPERS + 2; index += 1) {
    prefs = addRecentWallpaper(prefs, asset(String(index)));
  }
  prefs = addRecentWallpaper(prefs, asset("4"));
  assert.equal(prefs.recent.length, MAX_RECENT_WALLPAPERS);
  assert.equal(prefs.recent[0].id, "4");
  assert.equal(new Set(prefs.recent.map((item) => item.id)).size, prefs.recent.length);
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
  assert.equal(
    cycleRecentWallpaper({ ...prefs, current: second }).current?.id,
    "first",
  );
  assert.equal(
    cycleRecentWallpaper({ ...prefs, recent: [first] }).current?.id,
    "first",
  );
});
