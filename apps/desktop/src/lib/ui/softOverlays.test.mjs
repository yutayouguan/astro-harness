import assert from "node:assert/strict";
import test from "node:test";
import { readFile } from "node:fs/promises";
const read = (path) => readFile(new URL(path, import.meta.url), "utf8");
const css = await read("../../styles/materials/soft-overlays.css");

test("soft floating surfaces use solid fill and one neutral shadow", () => {
  for (const surface of [
    ".app-dialog",
    ".ui-overlay__surface",
    ".fs-ctx-menu",
    ".project-context-menu",
    ".astro-toast",
  ])
    assert.ok(css.includes(surface));
  assert.match(css, /background: var\(--soft-surface\)/);
  assert.match(css, /box-shadow: var\(--soft-floating-shadow\)/);
  assert.match(css, /backdrop-filter: none/);
  assert.doesNotMatch(
    css,
    /!important|z-index:|position:|animation:|transform:|pointer-events:|--toast-tone:/,
  );
});

test("anchored popovers and searchable results never receive a second surface", () => {
  assert.match(css, /\.ui-overlay:not\(\.ui-overlay--transparent\)/);
  assert.match(css, /\.select-menu-list:not\(\.select-menu-results\)/);
  assert.match(css, /\.select-menu-search\s*\{\s*border-radius: 8px;/);
});

test("danger and high contrast remain explicit while focus keeps final precedence", async () => {
  assert.match(
    css,
    /\.app-dialog-btn\.is-confirm\.is-danger[\s\S]*var\(--dialog-danger, var\(--danger\)\)/,
  );
  assert.match(css, /:hover:not\(\s*:disabled,\s*\.is-danger\s*\)/);
  assert.match(
    css,
    /prefers-contrast: more[\s\S]*--soft-floating-shadow: none/,
  );
  const index = await read("../../styles/index.css");
  assert.ok(
    index.indexOf("materials/soft-overlays.css") <
      index.indexOf("materials/soft-focus.css"),
  );
  assert.doesNotMatch(css, /outline:\s*none/);
});
