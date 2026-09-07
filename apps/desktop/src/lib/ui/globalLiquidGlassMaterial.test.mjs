import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const [intensity, segmented, sidebar, accessibility, index] = await Promise.all(
  [
    "../../styles/tokens/glass-intensity.css",
    "../../styles/tokens/component/segmented.css",
    "../../styles/features/shell/layout/sidebar-polish.css",
    "../../styles/tokens/a11y.css",
    "../../styles/index.css",
  ].map((path) => readFile(new URL(path, import.meta.url), "utf8")),
);

test("global chrome, surfaces, and content cards share one liquid glass recipe", () => {
  for (const alias of [
    "--sidebar-bg",
    "--composer-bg",
    "--menu-glass-bg",
    "--menu-overlay-bg",
    "--titlebar-menu-bg",
    "--header-chip-bg",
    "--lens-bg",
    "--badge-bg",
    "--content-card-background",
  ]) {
    assert.match(
      intensity,
      new RegExp(`${alias}: var\\(--global-liquid-glass-`),
      `${alias} must use the global liquid glass contract`,
    );
  }
});

test("segmented controls and sidebar chrome consume the global recipe", () => {
  assert.match(
    segmented,
    /--seg-shell-bg:\s*var\(\s*--global-liquid-glass-background/,
  );
  assert.match(
    segmented,
    /--seg-shell-shadow:\s*var\(\s*--global-liquid-glass-shadow/,
  );
  assert.match(
    sidebar,
    /--sidebar-chrome-sheen:\s*var\(\s*--liquid-glass-sheen/,
  );
  assert.match(
    sidebar,
    /--sidebar-chrome-filter:\s*var\(\s*--global-liquid-glass-backdrop/,
  );
});

test("intensity overrides load after theme-specific material recipes", () => {
  const glass = index.indexOf("tokens/component/glass.css");
  const menu = index.indexOf("tokens/component/menu.css");
  const header = index.indexOf("tokens/component/header.css");
  const intensityOverrides = index.indexOf("tokens/glass-intensity.css");

  assert.ok(glass >= 0);
  assert.ok(menu > glass);
  assert.ok(header > menu);
  assert.ok(intensityOverrides > header);
});

test("accessibility preferences replace the global recipe with a solid surface", () => {
  assert.match(
    accessibility,
    /@media \(prefers-reduced-transparency: reduce\)[\s\S]*?--global-liquid-glass-background:\s*var\(--surface-panel-background\);[\s\S]*?--global-liquid-glass-backdrop:\s*none;/,
  );
  assert.match(
    accessibility,
    /@media \(prefers-contrast: more\)[\s\S]*?--global-liquid-glass-border:\s*var\(--color-border\);[\s\S]*?--global-liquid-glass-backdrop:\s*none;/,
  );
});
