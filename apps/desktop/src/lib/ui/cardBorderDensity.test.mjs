import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const [glass, cardBorders, index, accessibility] = await Promise.all(
  [
    "../../styles/tokens/component/glass.css",
    "../../styles/components/card-borders.css",
    "../../styles/index.css",
    "../../styles/tokens/a11y.css",
  ].map((path) => readFile(new URL(path, import.meta.url), "utf8")),
);

test("semantic cards use the shared thin border width", () => {
  assert.match(glass, /--content-card-border-width:\s*0\.5px;/);
  assert.match(
    cardBorders,
    /:where\(\[class\$="-card"\], \[class\*="-card "\], \[class~="card"\], \.prefs-section\)/,
  );
  assert.match(
    cardBorders,
    /border-width:\s*var\(--content-card-border-width, 0\.5px\);/,
  );
  assert.match(
    index,
    /@import "\.\/components\/card-borders\.css" layer\(overrides\);/,
  );
});

test("higher contrast keeps a full-width card outline", () => {
  assert.match(
    accessibility,
    /@media \(prefers-contrast: more\)[\s\S]*?--content-card-border-width:\s*1px;/,
  );
});
