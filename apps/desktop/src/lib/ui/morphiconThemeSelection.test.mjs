import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const preferencesCss = await readFile(
  new URL("../../styles/features/preferences.css", import.meta.url),
  "utf8",
);
const segmentedTokens = await readFile(
  new URL("../../styles/tokens/component/segmented.css", import.meta.url),
  "utf8",
);

function rule(css, selector) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return css.match(
    new RegExp(`(?:^|\\n)${escaped}\\s*\\{(?<body>[\\s\\S]*?)\\}`),
  )?.groups?.body;
}

test("morphicon selection follows the local theme tone", () => {
  const active = rule(preferencesCss, ".morphicon-segmented button.is-active");

  assert.ok(active, "missing Morphicon selected-state styles");
  assert.match(active, /color:\s*var\(--seg-item-color-active\);/);
  assert.match(active, /background:\s*var\(--seg-item-bg-active\);/);
  assert.match(active, /box-shadow:\s*var\(--seg-item-shadow-active\);/);
  assert.doesNotMatch(active, /var\(--ink\)\s+92%/);
  assert.match(
    segmentedTokens,
    /:is\([\s\S]*?\.morphicon-segmented,[\s\S]*?\)\s*\{[\s\S]*?--seg-item-bg-active:/,
    "Morphicon controls must inherit the shared tone-aware segmented recipe",
  );
});
