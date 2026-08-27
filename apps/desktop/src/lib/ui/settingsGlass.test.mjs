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

test("active settings navigation uses the shared translucent glass material", () => {
  const sidebar = rule(
    projectStyles,
    ".sidebar-settings-nav .settings-sidebar-item.is-active",
  );
  const categories = rule(
    preferenceStyles,
    ".prefs-category-nav-item.is-active",
  );

  for (const body of [sidebar, categories]) {
    assert.ok(body, "missing active settings navigation rule");
    assert.match(body, /background:\s*var\(--glass-fill-soft\);/);
    assert.match(body, /border-color:\s*color-mix\(/);
    assert.match(body, /box-shadow:[\s\S]*var\(--glass-rim\)/);
    assert.match(body, /backdrop-filter:\s*var\(--backdrop-glass\);/);
    assert.match(body, /-webkit-backdrop-filter:\s*var\(--backdrop-glass\);/);
  }
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
