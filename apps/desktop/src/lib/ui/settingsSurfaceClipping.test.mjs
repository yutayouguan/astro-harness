import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const surfaces = await readFile(
  new URL("../../styles/features/settings-card-surfaces.css", import.meta.url),
  "utf8",
);

test("independent settings cards clip WebView backdrop layers to their radius", () => {
  assert.match(
    surfaces,
    /\.prefs-page\.is-embedded \.prefs-category-stack > \.prefs-card,[\s\S]*?isolation:\s*isolate;[\s\S]*?overflow:\s*hidden;[\s\S]*?background-clip:\s*padding-box;/,
  );
  assert.doesNotMatch(surfaces, /prefs-card--general|prefs-general-group/);
});
