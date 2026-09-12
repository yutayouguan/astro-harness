import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const read = (path) => readFile(new URL(path, import.meta.url), "utf8");
const [soft, controls, overlays] = await Promise.all([
  read("../../styles/materials/soft.css"),
  read("../../styles/materials/soft-controls.css"),
  read("../../styles/materials/soft-overlays.css"),
]);

test("Soft retains low relief without long drop shadows or bright full-pixel rims", () => {
  assert.match(soft, /--soft-raised-shadow:\s*0 2px 6px -3px var\(--soft-shade\), inset 0 0\.5px 0 var\(--soft-highlight\)/);
  assert.match(soft, /--soft-control-drop-shadow: 0 2px 5px -2px var\(--soft-shade\)/);
  assert.match(soft, /--soft-inset-shadow:\s*inset 0 1px 2px var\(--soft-shade\)/);
  assert.match(overlays, /--soft-floating-shadow: 0 8px 24px -12px var\(--soft-shade\)/);
  assert.doesNotMatch(soft, /0 4px [79]px|inset 0 2px 4px/);
});

test("light and dark decoration stay subtle while accessibility remains independent", () => {
  assert.match(soft, /--soft-shade: rgba\(48, 57, 75, 0\.08\)/);
  assert.match(soft, /--soft-shade: rgba\(0, 0, 0, 0\.18\)/);
  assert.match(controls, /var\(--soft-muted\) 16%/);
  assert.match(controls, /--select-glass-shadow: var\(--soft-control-drop-shadow\)/);
  assert.match(soft, /prefers-contrast: more\)\s*\{[^}]*--soft-control-drop-shadow: none/);
  assert.match(controls, /prefers-contrast: more[\s\S]*--soft-control-edge: var\(--soft-muted\)/);
});
