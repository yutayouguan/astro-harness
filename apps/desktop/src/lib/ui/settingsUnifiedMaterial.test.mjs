import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const [entry, preview, material] = await Promise.all(
  [
    "../../main.tsx",
    "../../../.storybook/preview.tsx",
    "../../styles/features/settings-material-unified.css",
  ].map((path) => readFile(new URL(path, import.meta.url), "utf8")),
);

test("unified settings material loads after the legacy style bundle", () => {
  assert.ok(
    entry.indexOf("./styles/features/settings-material-unified.css") >
      entry.indexOf("./styles/index.css"),
  );
  assert.ok(
    preview.indexOf("../src/styles/features/settings-material-unified.css") >
      preview.indexOf("../src/styles/index.css"),
  );
});

test("settings expose one shared inset and selected material contract", () => {
  for (const token of [
    "--settings-inset-border",
    "--settings-inset-background",
    "--settings-inset-background-strong",
    "--settings-inset-hover",
    "--settings-selected-border",
    "--settings-selected-background",
  ]) {
    assert.match(material, new RegExp(`${token}:`));
  }

  assert.match(material, /\.theme-option,/);
  assert.match(material, /\.appearance-segmented,/);
  assert.match(material, /\.terminal-mode-option,/);
  assert.match(material, /\.browser-viewport-options,/);
  assert.match(material, /\.prefs-context-stage/);
  assert.match(material, /\.aux-number-input,/);
  assert.match(material, /backdrop-filter:\s*none;/);
  assert.match(material, /\.prefs-card:not\(\.prefs-card--general\)/);
  assert.match(
    material,
    /\.prefs-card:not\(\.prefs-card--general\), \.prefs-section\),[\s\S]*?background-clip:\s*padding-box;[\s\S]*?backdrop-filter:\s*none;/,
  );
  assert.match(
    material,
    /\.settings-content-inline,\s*\.prefs-page\.is-embedded\s*\{/,
  );
});

test("selected and accessibility states keep the unified hierarchy", () => {
  assert.match(material, /\.theme-option\.active,/);
  assert.match(material, /\.terminal-mode-option\.is-active,/);
  assert.match(material, /\.browser-viewport-options button\.is-active/);
  assert.match(
    material,
    /@media \(prefers-reduced-transparency: reduce\)[\s\S]*?--settings-inset-background:\s*var\(--surface-panel-background\);/,
  );
  assert.match(
    material,
    /@media \(prefers-contrast: more\)[\s\S]*?--settings-inset-border:\s*var\(--color-border\);/,
  );
});
