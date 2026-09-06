import assert from "node:assert/strict";
import test from "node:test";

import {
  resolveWallpaperPresentation,
  tokensForResolvedTheme,
  type ActiveUiStyle,
} from "./activeUiStyle.ts";
import { DEFAULT_WALLPAPER_PREFS } from "./wallpaper.ts";

const style: ActiveUiStyle = {
  schemaVersion: 1,
  id: "aurora-night",
  name: "极光夜色",
  revision: "r1",
  updatedAt: "2026-09-06T12:00:00Z",
  tokens: {
    light: { "--color-accent": "#2563eb" },
    dark: { "--color-accent": "#7dd3fc" },
  },
  icons: { motion: "smooth", strokeWidth: 2 },
  wallpaper: {
    path: "/tmp/astro/theme/wallpaper.webp",
    fit: "contain",
    shade: 24,
    blur: 3,
    adaptiveColor: false,
    recommendedTheme: "dark",
  },
};

test("active style selects tokens for the resolved light or dark theme", () => {
  assert.deepEqual(tokensForResolvedTheme(style, "light"), {
    "--color-accent": "#2563eb",
  });
  assert.deepEqual(tokensForResolvedTheme(style, "dark"), {
    "--color-accent": "#7dd3fc",
  });
  assert.deepEqual(tokensForResolvedTheme(null, "dark"), {});
});

test("generated wallpaper overrides manual prefs and reset falls back", () => {
  const manual = {
    ...DEFAULT_WALLPAPER_PREFS,
    mode: "wallpaper" as const,
    fit: "cover" as const,
    current: {
      id: "manual",
      path: "/tmp/manual.png",
      name: "Manual",
      source: "upload" as const,
      width: 10,
      height: 10,
      createdAt: "now",
    },
  };
  const active = resolveWallpaperPresentation(style, manual);
  assert.equal(active.generated, true);
  assert.equal(active.wallpaper?.path, style.wallpaper?.path);
  assert.equal(active.fit, "contain");
  assert.equal(active.adaptiveColor, false);

  const fallback = resolveWallpaperPresentation(null, manual);
  assert.equal(fallback.generated, false);
  assert.equal(fallback.wallpaper?.path, "/tmp/manual.png");
  assert.equal(fallback.fit, "cover");
});
