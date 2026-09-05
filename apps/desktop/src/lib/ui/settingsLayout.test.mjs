import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const [
  appShellCss,
  preferencesCss,
  terminalCss,
  browserCss,
  preferences,
  terminal,
  compression,
] = await Promise.all(
  [
    "../../styles/features/shell/layout/projects.css",
    "../../styles/features/preferences.css",
    "../../styles/features/terminal-settings.css",
    "../../styles/features/browser-settings.css",
    "../../components/settings/PreferencesPanel.tsx",
    "../../components/settings/TerminalSettingsPanel.tsx",
    "../../components/settings/CompressionSettingsCard.tsx",
  ].map((path) => readFile(new URL(path, import.meta.url), "utf8")),
);

test("preference-backed settings use one scroll owner and one content width", () => {
  assert.match(
    appShellCss,
    /\.settings-content-inline:has\(> \.prefs-page\)[\s\S]*?overflow:\s*hidden;/,
  );
  assert.match(
    preferencesCss,
    /\.prefs-page\.is-embedded\s*\{[\s\S]*?overflow:\s*hidden;/,
  );
  assert.match(
    preferencesCss,
    /\.prefs-page\.is-embedded \.prefs-category-content\s*\{[\s\S]*?max-width:\s*920px;/,
  );
  assert.doesNotMatch(terminalCss, /max-width:\s*920px/);
  assert.doesNotMatch(browserCss, /max-width:\s*760px/);
});

test("each preference-backed tab owns one shared glass surface", () => {
  assert.match(
    preferencesCss,
    /\.prefs-page\.is-embedded \.prefs-category-stack\s*\{[\s\S]*?background:\s*var\(--settings-panel-background[\s\S]*?backdrop-filter:\s*var\(--settings-panel-backdrop/,
  );
  assert.match(
    preferencesCss,
    /\.prefs-page\.is-embedded \.prefs-category-stack > \.prefs-card,[\s\S]*?background:\s*transparent;[\s\S]*?backdrop-filter:\s*none;/,
  );
});

test("appearance and conversation use responsive grouped layouts", () => {
  assert.match(preferences, /prefs-category-stack--appearance/);
  assert.match(preferences, /prefs-card--conversation-display/);
  assert.match(
    preferencesCss,
    /\.prefs-category-stack--appearance:not\(\[hidden\]\)[\s\S]*?grid-template-columns:\s*repeat\(2,/,
  );
  assert.match(
    preferencesCss,
    /\.prefs-card--conversation-display \.prefs-toggle-list[\s\S]*?grid-template-columns:\s*repeat\(2,/,
  );

  const conversation = preferences.slice(
    preferences.indexOf('hidden={activeCategory !== "conversation"}'),
    preferences.indexOf('hidden={activeCategory !== "context"}'),
  );
  const general = preferences.slice(
    preferences.indexOf('hidden={activeCategory !== "general"}'),
    preferences.indexOf('hidden={activeCategory !== "about"}'),
  );
  assert.doesNotMatch(conversation, /<SidebarVisibleSetting/);
  assert.match(general, /<SidebarVisibleSetting/);
});

test("terminal settings keep controls left and a responsive live preview right", () => {
  assert.match(
    terminal,
    /className="prefs-category-stack terminal-settings-layout"/,
  );
  assert.match(terminal, /<aside className="terminal-settings-preview-pane">/);
  assert.match(
    terminalCss,
    /\.terminal-settings-layout:not\(\[hidden\]\)[\s\S]*?grid-template-columns:\s*minmax\(0, 1fr\) minmax\(250px, 0\.72fr\);/,
  );
  assert.match(
    terminalCss,
    /\.terminal-settings-preview-pane[\s\S]*?grid-column:\s*2;/,
  );
});

test("automatic compression hides expert controls behind native disclosure", () => {
  assert.match(compression, /<details className="prefs-context-advanced">/);
  assert.match(compression, /<summary>/);
  assert.ok(
    compression.indexOf("prefs.context.stages") <
      compression.indexOf('className="prefs-context-advanced"'),
  );
});

test("diagnostics and about have task-specific layouts", () => {
  assert.match(preferences, /prefs-category-stack--diagnostics/);
  assert.match(preferences, /prefs-card--diagnostics/);
  assert.match(preferences, /prefs-category-stack--about/);
  assert.match(preferences, /prefs-about-version/);
  assert.match(preferences, /ABOUT_FEATURES\.map/);
  assert.match(
    preferencesCss,
    /\.prefs-card--diagnostics\s*\{[\s\S]*?flex:\s*1;/,
  );
  assert.match(
    preferencesCss,
    /\.prefs-diag-log\s*\{[\s\S]*?flex:\s*1;[\s\S]*?overflow:\s*auto;/,
  );
  assert.match(
    preferencesCss,
    /\.prefs-category-stack--about:not\(\[hidden\]\)[\s\S]*?width:\s*min\(100%, 620px\);/,
  );
});
