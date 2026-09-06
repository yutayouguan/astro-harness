import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const [indexCss, surfacesCss, terminal, terminalCss, browser, browserCss] =
  await Promise.all(
    [
      "../../styles/index.css",
      "../../styles/features/settings-card-surfaces.css",
      "../../components/settings/TerminalSettingsPanel.tsx",
      "../../styles/features/terminal-settings.css",
      "../../components/settings/BrowserSettingsPanel.tsx",
      "../../styles/features/browser-settings.css",
    ].map((path) => readFile(new URL(path, import.meta.url), "utf8")),
  );

test("settings card overrides load between shared and page-specific styles", () => {
  assert.ok(
    indexCss.indexOf("./features/settings-card-surfaces.css") >
      indexCss.indexOf("./features/preferences.css"),
  );
  assert.ok(
    indexCss.indexOf("./features/settings-card-surfaces.css") <
      indexCss.indexOf("./features/browser-settings.css"),
  );
  assert.match(
    surfacesCss,
    /\.prefs-page\.is-embedded \.prefs-category-stack\s*\{[\s\S]*?gap:\s*14px;[\s\S]*?background:\s*transparent;/,
  );
  assert.match(
    surfacesCss,
    /\.prefs-page\.is-embedded \.prefs-category-stack > \.prefs-card,[\s\S]*?border-radius:\s*var\(--settings-panel-radius[\s\S]*?background:\s*var\(--settings-panel-background/,
  );
});

test("general preferences expose language and system as separate cards", () => {
  assert.match(
    surfacesCss,
    /\.prefs-category-stack--general[\s\S]*?> \.prefs-card--general[\s\S]*?background:\s*transparent;/,
  );
  assert.match(
    surfacesCss,
    /\.prefs-category-stack--general \.prefs-general-group\s*\{[\s\S]*?border-radius:\s*var\(--settings-panel-radius/,
  );
  assert.match(
    surfacesCss,
    /\.prefs-category-stack--general \.prefs-general-divider\s*\{[\s\S]*?display:\s*none;/,
  );
});

test("terminal and browser settings keep independent functional sections", () => {
  assert.match(terminal, /terminal-settings-section--mode/);
  assert.match(terminal, /terminal-settings-section--font/);
  assert.match(terminal, /terminal-settings-section--behavior/);
  assert.match(terminal, /TerminalSquare/);
  assert.match(terminal, /CornerDownLeft/);
  assert.match(terminal, /terminal-mode-options/);
  assert.match(terminal, /terminal-cursor-options/);
  assert.match(
    terminalCss,
    /\.terminal-settings-layout:not\(\[hidden\]\)[\s\S]*?grid-template-columns:\s*repeat\(2,/,
  );
  assert.ok(
    (browser.match(/<section className="prefs-card/g) ?? []).length >= 3,
    "browser settings should keep its runtime, startup, and permission cards",
  );
  assert.match(browser, /browser-settings-layout/);
  assert.match(browser, /browser-settings-card--runtime/);
  assert.match(browser, /browser-settings-card--startup/);
  assert.match(browser, /browser-settings-card--permissions/);
  assert.match(browser, /browser-settings-card--sites/);
  assert.match(
    browserCss,
    /\.browser-settings-layout:not\(\[hidden\]\)[\s\S]*?grid-template-columns:\s*repeat\(2,/,
  );
  assert.match(
    browserCss,
    /\.browser-settings-card--permissions,[\s\S]*?\.browser-settings-card--sites\s*\{[\s\S]*?grid-column:\s*1 \/ -1;/,
  );
});
