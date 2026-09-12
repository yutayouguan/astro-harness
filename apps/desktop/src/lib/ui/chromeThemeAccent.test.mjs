import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
const read = (path) => readFile(new URL(path, import.meta.url), "utf8");
const [footer, header, unified] = await Promise.all([
  read("../../styles/features/shell/layout/sidebar-footer-actions.css"),
  read("../../styles/features/shell/header.css"),
  read("../../styles/tokens/unified-color.css"),
]);

test("pet active and keyboard focus colors prefer the live theme tone", () => {
  assert.match(
    footer,
    /\[aria-pressed="true"\]\s*\{\s*color: var\(--tone, var\(--accent, var\(--ink\)\)\)/,
  );
  assert.match(footer, /:focus-visible\s*\{\s*outline: 2px solid var\(--tone,/);
  assert.match(footer, /:disabled\s*\{\s*opacity: 0\.45/);
});

test("unified and dynamic palettes reach both the model trigger and body portal", () => {
  for (const selector of [".model-picker,", ".model-picker-flyout,"]) {
    assert.ok(
      unified.includes(
        `html:is([data-color-style="unified"], [data-color-style="dynamic"]) ${selector}`,
      ),
    );
  }
  assert.match(
    header,
    /\.model-picker,\s*\.model-picker-flyout\s*\{\s*--accent: var\(--tone\)/,
  );
  assert.match(
    header,
    /\.model-picker-option-check\.is-visible\s*\{\s*color: var\(--tone, var\(--accent\)\)/,
  );
  assert.match(
    unified,
    /html\[data-wallpaper-palette="true"\][\s\S]*--unified-tone: var\(--wallpaper-tone/,
  );
});
