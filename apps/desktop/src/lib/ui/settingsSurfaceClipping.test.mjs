import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const surfaces = await readFile(
  new URL("../../styles/features/settings-card-surfaces.css", import.meta.url),
  "utf8",
);

test("settings stack stays transparent and does not wrap card surfaces", () => {
  assert.match(
    surfaces,
    /\.prefs-page\.is-embedded \.prefs-category-stack\s*\{[\s\S]*?background:\s*transparent;[\s\S]*?backdrop-filter:\s*none;/,
  );
  assert.doesNotMatch(surfaces, /prefs-card--general|prefs-general-group/);
  assert.doesNotMatch(surfaces, /> \.prefs-card|> \.prefs-section/);
});
