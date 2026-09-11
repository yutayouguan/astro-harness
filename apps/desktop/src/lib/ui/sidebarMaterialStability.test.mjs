import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const read = (path) => readFileSync(new URL(path, import.meta.url), "utf8");

test("palette updates never tear down the glass material between frames", () => {
  for (const path of [
    "../../App.tsx",
    "./shellGradient.ts",
    "../../styles/tokens/unified-color.css",
  ]) {
    assert.doesNotMatch(
      read(path),
      /flushGlassBackdrop|data-glass-flush/,
      path,
    );
  }
});

test("sidebar continues to share the header glass rather than hiding the flash with an opaque fill", () => {
  const css = read("../../styles/features/shell/layout/sidebar-polish.css");
  assert.match(css, /--sidebar-chrome-background: var\(--titlebar-menu-bg\)/);
  assert.match(css, /backdrop-filter: var\(--sidebar-chrome-filter\)/);
});
