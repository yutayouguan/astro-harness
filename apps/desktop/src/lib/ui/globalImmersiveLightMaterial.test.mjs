import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const [immersive, segmented, sidebar, accessibility, index] = await Promise.all(
  [
    "../../styles/tokens/immersive-light.css",
    "../../styles/tokens/component/segmented.css",
    "../../styles/features/shell/layout/sidebar-polish.css",
    "../../styles/tokens/a11y.css",
    "../../styles/index.css",
  ].map((path) => readFile(new URL(path, import.meta.url), "utf8")),
);

test("global chrome, surfaces, and content cards share one immersive recipe", () => {
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
      immersive,
      new RegExp(`${alias}: var\\(--immersive-`),
      `${alias} must use the immersive light contract`,
    );
  }
});

test("segmented controls and sidebar chrome consume the global recipe", () => {
  assert.match(
    segmented,
    /--seg-shell-bg:\s*var\(\s*--immersive-glass-background/,
  );
  assert.match(
    segmented,
    /--seg-shell-shadow:\s*var\(\s*--immersive-glass-shadow/,
  );
  assert.match(
    sidebar,
    /--sidebar-chrome-sheen:\s*var\(\s*--immersive-glass-light-field/,
  );
  assert.match(
    sidebar,
    /--sidebar-chrome-filter:\s*var\(\s*--immersive-glass-backdrop/,
  );
});

test("immersive material loads after component fallback recipes", () => {
  const glass = index.indexOf("tokens/component/glass.css");
  const menu = index.indexOf("tokens/component/menu.css");
  const header = index.indexOf("tokens/component/header.css");
  const immersiveOverrides = index.indexOf("tokens/immersive-light.css");

  assert.ok(glass >= 0);
  assert.ok(menu > glass);
  assert.ok(header > menu);
  assert.ok(immersiveOverrides > header);
  assert.doesNotMatch(index, /liquid-glass\.css|glass-intensity\.css/);
});

test("accessibility preferences replace the global recipe with a solid surface", () => {
  assert.match(
    accessibility,
    /@media \(prefers-reduced-transparency: reduce\)[\s\S]*?--immersive-glass-background:\s*var\(--surface-panel-background\);[\s\S]*?--immersive-glass-backdrop:\s*none;/,
  );
  assert.match(
    accessibility,
    /@media \(prefers-contrast: more\)[\s\S]*?--immersive-glass-border:\s*var\(--color-border\);[\s\S]*?--immersive-glass-backdrop:\s*none;/,
  );
});
