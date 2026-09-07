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

test("unified settings material loads after shared feature styles", () => {
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
    "--settings-panel-border",
    "--settings-panel-background",
    "--settings-panel-shadow",
    "--settings-panel-backdrop",
  ]) {
    assert.match(
      material,
      new RegExp(`${token}: var\\(\\s*--immersive-glass-`),
      `${token} must bind directly to immersive glass`,
    );
  }

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
  assert.match(material, /> :is\(\.prefs-card, \.prefs-section\)/);
  assert.match(
    material,
    /> :is\(\.prefs-card, \.prefs-section\)\s*\{[\s\S]*?border:\s*1px solid[\s\S]*?border-radius:\s*var\(--settings-panel-radius,[\s\S]*?background-clip:\s*padding-box;[\s\S]*?box-shadow:\s*var\(\s*--settings-panel-shadow,[\s\S]*?backdrop-filter:\s*none;/,
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
    /@media \(prefers-reduced-transparency: reduce\)[\s\S]*?--settings-panel-background:\s*var\(--surface-panel-background\);[\s\S]*?--settings-panel-backdrop:\s*none;[\s\S]*?--settings-inset-background:\s*var\(--surface-panel-background\);/,
  );
  assert.match(
    material,
    /@media \(prefers-contrast: more\)[\s\S]*?--settings-panel-border:\s*var\(--color-border\);[\s\S]*?--settings-panel-backdrop:\s*none;[\s\S]*?--settings-inset-border:\s*var\(--color-border\);/,
  );
});

test("all settings surface families use the neutral appearance glass plane", () => {
  for (const selector of [
    ".providers-pane",
    ".tools-detail-panel",
    ".approvals-section",
    ".tool-card.agent-tool-card",
    ".mem-card",
    ".model-market-card",
    ".mm-rank-surface",
    ".insights-kpi",
  ]) {
    assert.match(material, new RegExp(selector.replaceAll(".", "\\.")));
  }

  assert.match(
    material,
    /\.settings-content-inline[\s\S]*?:is\([\s\S]*?\.providers-pane,[\s\S]*?\.tool-card\.agent-tool-card,[\s\S]*?\.mem-card,[\s\S]*?\.model-market-card,[\s\S]*?\.insights-kpi,[\s\S]*?\)\s*\{[\s\S]*?background:\s*var\(--settings-panel-background\);[\s\S]*?backdrop-filter:\s*none;/,
  );
  assert.match(
    material,
    /:is\(\.providers-pane, \.mcp-server-card\)::before\s*\{[\s\S]*?content:\s*none;/,
  );
});

test("composite settings controls do not create nested glass shells", () => {
  assert.ok(
    material.indexOf(".browser-permission-toggles {") >
      material.indexOf(".prefs-toggle-list,"),
    "browser permission flattening must follow the generic inset material",
  );
  assert.match(
    material,
    /\.mm-rankings-commandbar[\s\S]*?> \.mm-rankings-sections\s*\{[\s\S]*?border:\s*0;[\s\S]*?background:\s*transparent;[\s\S]*?box-shadow:\s*none;[\s\S]*?backdrop-filter:\s*none;/,
  );
  assert.match(
    material,
    /\.browser-permission-toggles\s*\{[\s\S]*?border:\s*0;[\s\S]*?background:\s*transparent;[\s\S]*?box-shadow:\s*none;[\s\S]*?backdrop-filter:\s*none;/,
  );
  assert.match(
    material,
    /\.browser-permission-toggles[\s\S]*?> \.prefs-toggle-row\s*\{[\s\S]*?border-top:\s*1px solid var\(--settings-divider\);[\s\S]*?border-radius:\s*0;/,
  );
});
