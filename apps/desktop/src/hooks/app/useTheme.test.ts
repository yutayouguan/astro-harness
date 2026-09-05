import assert from "node:assert/strict";
import { test } from "node:test";

import { resolveThemePreference } from "../../lib/ui/themeResolution.ts";

test("auto theme follows analyzed wallpaper before the system preference", () => {
  assert.equal(resolveThemePreference("auto", false, "dark"), "dark");
  assert.equal(resolveThemePreference("auto", true, "light"), "light");
  assert.equal(resolveThemePreference("auto", true, null), "dark");
  assert.equal(resolveThemePreference("auto", false, null), "light");
});

test("explicit user theme is never overridden by wallpaper analysis", () => {
  assert.equal(resolveThemePreference("light", true, "dark"), "light");
  assert.equal(resolveThemePreference("dark", false, "light"), "dark");
});
