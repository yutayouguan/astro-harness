import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const preferencesCss = await readFile(
  new URL("../../styles/features/preferences.css", import.meta.url),
  "utf8",
);

function rule(css, selector) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return css.match(
    new RegExp(`(?:^|\\n)${escaped}\\s*\\{(?<body>[\\s\\S]*?)\\}`),
  )?.groups?.body;
}

test("morphicon selection matches the appearance option card treatment", () => {
  const active = rule(preferencesCss, ".morphicon-segmented button.is-active");
  const darkActive = rule(
    preferencesCss,
    'html[data-theme="dark"] .morphicon-segmented button.is-active',
  );

  assert.ok(active, "missing Morphicon selected-state styles");
  assert.match(active, /var\(--tone-soft\) 70%/);
  assert.match(active, /var\(--tone\) 36%/);
  assert.match(active, /border-radius:\s*10px/);
  assert.match(active, /font-weight:\s*650/);
  assert.match(active, /inset 0 1px 0 rgba\(255, 255, 255, 0\.35\)/);
  assert.doesNotMatch(active, /var\(--ink\)\s+92%/);
  assert.ok(darkActive, "missing dark-theme Morphicon selected-state styles");
  assert.match(darkActive, /var\(--tone-soft\) 55%/);
  assert.match(darkActive, /var\(--tone\) 40%/);
});
