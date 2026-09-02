import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const projectStyles = await readFile(
  new URL("../../styles/features/shell/layout/projects.css", import.meta.url),
  "utf8",
);
const preferenceStyles = await readFile(
  new URL("../../styles/features/preferences.css", import.meta.url),
  "utf8",
);

test("settings tabs share a responsive horizontal safe area", () => {
  assert.match(
    projectStyles,
    /\.settings-content-inline\s*\{[\s\S]*?--settings-content-inline-padding:\s*clamp\(20px,\s*2vw,\s*28px\);[\s\S]*?padding-inline:\s*var\(--settings-content-inline-padding\);/,
  );
});

test("embedded preferences rely on the shared safe area without doubling it", () => {
  assert.match(
    preferenceStyles,
    /\.prefs-page\.is-embedded\s+\.prefs-category-content\s*\{[\s\S]*?padding:\s*18px\s+0;/,
  );
});
