import assert from "node:assert/strict";
import test from "node:test";
import { readFile } from "node:fs/promises";

const read = (path) => readFile(new URL(path, import.meta.url), "utf8");
const [css, settings, index] = await Promise.all([
  read("../../styles/materials/soft-controls.css"),
  read("../../styles/features/settings-material-unified.css"),
  read("../../styles/index.css"),
]);

test("soft chrome keeps dock geometry but drops the raised surface shadow", () => {
  assert.match(css, /--chat-dock-surface-background: var\(--soft-material-background\)/);
  assert.match(css, /--chat-dock-surface-shadow: none/);
  assert.doesNotMatch(
    css,
    /(?:^|[;{])\s*(?:width|height|padding|margin|border-radius):|!important/,
  );
});

test("soft settings opt into opaque inset and local selection tokens; glass retains fallbacks", () => {
  for (const token of [
    "material-inset-background",
    "material-inset-background-strong",
    "material-selected-background",
    "material-selected-border",
    "material-selected-shadow",
  ]) {
    assert.match(css, new RegExp(`--${token}:`));
    assert.match(settings, new RegExp(`var\\(\\s*--${token},`));
  }
  assert.match(settings, /var\(--surface-panel-background\) 68%, transparent/);
  assert.match(css, /--material-selected-background: var\(--soft-material-background\)/);
});

test("control styling never paints inner search fields or overrides active input rings", () => {
  assert.match(
    css,
    /\.select-menu-trigger:hover:not\(:disabled, \[data-input-focus\]\)/,
  );
  assert.doesNotMatch(css, /\.composer-input|\.expandable-search-input/);
  assert.ok(
    index.indexOf("materials/soft-controls.css") <
      index.indexOf("materials/soft-focus.css"),
  );
});

test("segmented selection removes glass sheen and glow while preserving readable text", () => {
  assert.match(css, /--seg-item-sheen-a: transparent/);
  assert.match(css, /--seg-item-sheen-b: transparent/);
  assert.match(css, /--seg-item-glow-amt: 0%/);
  assert.match(css, /--seg-item-ink: var\(--soft-ink\)/);
  assert.match(css, /--soft-selection-width: 0\.5px/);
  assert.match(css, /prefers-contrast: more[\s\S]*--soft-selection-width: 1px/);
});

test("tabs and provider selections share a single thin inset edge", () => {
  assert.match(
    css,
    /--seg-item-bg-active: var\(--material-selected-background\)/,
  );
  assert.match(
    css,
    /--seg-item-shadow-active: var\(--material-selected-shadow\)/,
  );
  assert.match(
    css,
    /\.providers-list-item\.is-selected\s*\{[^}]*border-color: transparent;/,
  );
  assert.match(
    css,
    /--select-glass-shadow: 0 4px 7px -2px var\(--soft-shade\)/,
  );
  assert.match(css, /\.select-menu-trigger\s*\{\s*box-shadow: none;/);
});
