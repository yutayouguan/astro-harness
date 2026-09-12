import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const read = (path) => readFile(new URL(path, import.meta.url), "utf8");
const [soft, controls, settings] = await Promise.all([
  read("../../styles/materials/soft.css"),
  read("../../styles/materials/soft-controls.css"),
  read("../../styles/features/settings-material-unified.css"),
]);

test("milky material preserves atmosphere colors without frosting the wallpaper image or shade", () => {
  assert.doesNotMatch(soft, /--shell-bg:\s*var\(--soft-base\)/);
  assert.match(
    soft,
    /\.app-shell\s*\{[^}]*background:[^}]*var\(--shell-bg, var\(--soft-base\)\)/,
  );
  assert.match(
    soft,
    /linear-gradient\(var\(--soft-shell-veil\), var\(--soft-shell-veil\)\)/,
  );
  assert.doesNotMatch(
    soft,
    /\.shell-wallpaper-layer|--wallpaper-shade:|--wallpaper-blur:/,
  );
});

test("panels, milk buttons and input surfaces follow frost at distinct thicknesses", () => {
  assert.match(
    soft,
    /--soft-control-background: color-mix\(\s*in srgb,\s*var\(--soft-surface\)\s+calc\(100% - \(100% - var\(--soft-frost-opacity, 79%\)\) \* 2 \/ 7\),\s*transparent/,
  );
  assert.match(
    soft,
    /--soft-input-background: color-mix\(\s*in srgb,\s*var\(--soft-surface\)\s+calc\(100% - \(100% - var\(--soft-frost-opacity, 79%\)\) \* 4 \/ 7\),\s*transparent/,
  );
  assert.match(
    soft,
    /\.composer:not\(\.has-clarify\)\s*\{\s*background: var\(--soft-input-background\)/,
  );
  assert.match(controls, /--select-glass-fill: var\(--soft-input-background\)/);
  assert.match(
    controls,
    /\.ui-button--secondary:not\(\.is-active\)\s*\{\s*background: var\(--soft-control-background\)/,
  );
  assert.match(
    settings,
    /\.prefs-diag-btn:not\(\.danger\)\s*\{\s*background: var\(--soft-control-background\)/,
  );
});

test("zero frost and accessibility make every material solid without touching foreground opacity", () => {
  for (const block of [
    soft.match(/\[data-soft-frost-intensity="0"\]\s*\{([^}]+)/)?.[1],
    soft.match(
      /forced-colors: active[\s\S]*?html\[data-material="soft"\]\s*\{([^}]+)/,
    )?.[1],
  ]) {
    assert.ok(block);
    assert.match(block, /--soft-material-backdrop: none/);
    assert.match(block, /--soft-control-background: var\(--soft-surface\)/);
    assert.match(block, /--soft-input-background: var\(--soft-surface\)/);
  }
  assert.doesNotMatch(soft, /(?:^|[;{])\s*opacity:/m);
  assert.match(
    soft,
    /forced-colors: active[\s\S]*--surface-panel-background: var\(--soft-surface\)/,
  );
  assert.match(
    soft,
    /forced-colors: active[\s\S]*--titlebar-menu-bg: var\(--soft-surface\)/,
  );
});
