import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const projectStyles = await readFile(
  new URL("../../styles/features/shell/layout/projects.css", import.meta.url),
  "utf8",
);
const preferenceStyles = await readFile(
  new URL("../../styles/features/preferences.css", import.meta.url),
  "utf8",
);
const a11yStyles = await readFile(
  new URL("../../styles/tokens/a11y.css", import.meta.url),
  "utf8",
);

function rule(css, selector) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return css.match(new RegExp(`${escaped}\\s*\\{(?<body>[\\s\\S]*?)\\}`))?.groups?.body;
}

test("active sidebar settings navigation uses a flat tinted selection", () => {
  const sidebar = rule(
    projectStyles,
    ".sidebar-settings-nav .settings-sidebar-item.is-active",
  );

  assert.ok(sidebar, "missing active settings navigation rule");
  assert.match(sidebar, /background:\s*var\(--tone-soft/);
  assert.match(sidebar, /border-color:\s*color-mix\([\s\S]*?var\(--tone/);
  assert.match(sidebar, /box-shadow:\s*none;/);
  assert.match(sidebar, /backdrop-filter:\s*none;/);
  assert.doesNotMatch(sidebar, /var\(--glass-rim\)/);
});

test("settings navigation hover and press states stay flat", () => {
  const hover = rule(
    projectStyles,
    ".sidebar-settings-nav .settings-sidebar-item:hover",
  );
  const pressed = rule(
    projectStyles,
    ".sidebar-settings-nav .settings-sidebar-item:active",
  );

  assert.ok(hover, "missing settings navigation hover rule");
  assert.match(hover, /border-color:\s*transparent;/);
  assert.match(hover, /background:\s*transparent;/);
  assert.match(hover, /box-shadow:\s*none;/);
  assert.ok(pressed, "missing settings navigation pressed rule");
  assert.match(pressed, /transform:\s*none;/);
});

test("preference category navigation keeps the shared glass material", () => {
  const categories = rule(
    preferenceStyles,
    ".prefs-category-nav-item.is-active",
  );

  assert.ok(categories, "missing active preference category navigation rule");
  assert.match(categories, /background:\s*var\(--glass-fill-soft\);/);
  assert.match(categories, /border-color:\s*color-mix\(/);
  assert.match(categories, /box-shadow:[\s\S]*var\(--glass-rim\)/);
  assert.match(categories, /backdrop-filter:\s*var\(--backdrop-glass\);/);
  assert.match(categories, /-webkit-backdrop-filter:\s*var\(--backdrop-glass\);/);
});

test("reduced transparency replaces settings glass with a solid surface", () => {
  const media = a11yStyles.match(
    /@media \(prefers-reduced-transparency: reduce\)\s*\{(?<body>[\s\S]*?)\n\}/,
  )?.groups?.body;

  assert.ok(media, "missing reduced-transparency rules");
  assert.match(media, /\.settings-sidebar-item\.is-active/);
  assert.match(media, /\.prefs-category-nav-item\.is-active/);
  assert.match(media, /background:\s*var\(--glass-panel\);/);
});
