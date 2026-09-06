import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const [prototype, styles] = await Promise.all([
  readFile(
    new URL(
      "../../../../../designs/astro-wallpaper/settings-app.jsx",
      import.meta.url,
    ),
    "utf8",
  ),
  readFile(
    new URL(
      "../../../../../designs/astro-wallpaper/settings-prototype.css",
      import.meta.url,
    ),
    "utf8",
  ),
]);

test("settings prototype window supports bounded pointer dragging", () => {
  assert.match(prototype, /function beginWindowDrag\(event\)/);
  assert.match(prototype, /setPointerCapture\(event\.pointerId\)/);
  assert.match(prototype, /function moveWindow\(event\)/);
  assert.match(prototype, /window\.innerWidth - 140/);
  assert.match(prototype, /window\.innerHeight - 44/);
  assert.match(prototype, /function endWindowDrag\(event\)/);
  assert.match(prototype, /releasePointerCapture\(event\.pointerId\)/);
  assert.match(prototype, /onDoubleClick=\{\(\) => setWindowOffset/);
});

test("prototype titlebar exposes grab feedback without selecting text", () => {
  assert.match(
    styles,
    /\.titlebar\s*\{[\s\S]*?cursor:\s*grab;[\s\S]*?touch-action:\s*none;[\s\S]*?user-select:\s*none;/,
  );
  assert.match(
    styles,
    /\.app-window\.is-dragging \.titlebar\s*\{[\s\S]*?cursor:\s*grabbing;/,
  );
});
