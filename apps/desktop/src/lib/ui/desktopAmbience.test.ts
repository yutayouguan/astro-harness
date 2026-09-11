import { test } from "node:test";
import assert from "node:assert/strict";
import {
  ambiencePreferenceKey,
  ambienceWallpaperChoices,
  explicitStylePalette,
  materializeWallpaper,
  paletteTokens,
} from "./desktopAmbience.ts";
import { DEFAULT_WALLPAPER_PREFS } from "./wallpaper.ts";
import type { ActiveUiStyle } from "./activeUiStyle.ts";
const style: ActiveUiStyle = {
  schemaVersion: 1,
  id: "pet-room",
  name: "Room",
  revision: "r1",
  updatedAt: "now",
  icons: {},
  tokens: paletteTokens("#123456", "#789abc"),
  wallpaper: {
    path: "/ui/room.png",
    fit: "contain",
    shade: 27,
    blur: 3,
    adaptiveColor: false,
  },
};
test("detaching a scene retains the visible wallpaper and custom palette", () => {
  const prefs = materializeWallpaper(DEFAULT_WALLPAPER_PREFS, style, "dark");
  assert.equal(prefs.current?.path, "/ui/room.png");
  assert.equal(prefs.fit, "contain");
  assert.equal(prefs.shade, 27);
  assert.equal(prefs.blur, 3);
  assert.equal(prefs.adaptiveColor, false);
  assert.equal(prefs.customThemeColor, "#123456");
  assert.equal(prefs.followSystemWallpaper, false);
  assert.equal(DEFAULT_WALLPAPER_PREFS.current, null);
});
test("saved manual scene colors survive reapplication", () => {
  assert.deepEqual(explicitStylePalette(style, "light"), {
    themeColor: "#123456",
    highlightColor: "#789abc",
  });
  assert.equal(
    explicitStylePalette(
      { ...style, tokens: { light: {}, dark: {} } },
      "light",
    ),
    null,
  );
  assert.throws(() => paletteTokens("javascript:red", "#ffffff"));
});
test("analysis does not invalidate undo but selection changes do", () => {
  const before = materializeWallpaper(DEFAULT_WALLPAPER_PREFS, style, "light");
  const enriched = {
    ...before,
    current: { ...before.current!, accentColor: "#abcdef" },
  };
  assert.equal(ambiencePreferenceKey(before), ambiencePreferenceKey(enriched));
  assert.notEqual(
    ambiencePreferenceKey(before),
    ambiencePreferenceKey({ ...before, adaptiveColor: true }),
  );
});
test("wallpaper choices do not duplicate the same path", () => {
  const asset = materializeWallpaper(DEFAULT_WALLPAPER_PREFS, style, "light")
    .current!;
  assert.equal(ambienceWallpaperChoices([asset, asset], []).length, 1);
});
