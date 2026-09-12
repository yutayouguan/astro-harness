import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const [
  appShellCss,
  preferencesCss,
  settingsCardCss,
  settingsMaterialCss,
  terminalCss,
  browserCss,
  preferences,
  chatDisplayPrefs,
  terminal,
  compression,
  updater,
  prototypePages,
  prototypeCss,
  diagnosticsHook,
  diagnosticsModel,
] = await Promise.all(
  [
    "../../styles/features/shell/layout/projects.css",
    "../../styles/features/preferences.css",
    "../../styles/features/settings-card-surfaces.css",
    "../../styles/features/settings-material-unified.css",
    "../../styles/features/terminal-settings.css",
    "../../styles/features/browser-settings.css",
    "../../components/settings/PreferencesPanel.tsx",
    "../../hooks/chat/useChatDisplayPrefs.ts",
    "../../components/settings/TerminalSettingsPanel.tsx",
    "../../components/settings/CompressionSettingsCard.tsx",
    "../../../src-tauri/src/commands/ui/updater.rs",
    "../../../../../designs/astro-wallpaper/settings-pages.jsx",
    "../../../../../designs/astro-wallpaper/settings-prototype.css",
    "../../hooks/settings/useDiagnosticsSettings.ts",
    "../settings/diagnosticsModel.ts",
  ].map((path) => readFile(new URL(path, import.meta.url), "utf8")),
);

test("chat display presets define process visibility and default expansion", () => {
  const normalPreset = chatDisplayPrefs.slice(
    chatDisplayPrefs.indexOf("normal: {"),
    chatDisplayPrefs.indexOf("detailed: {"),
  );
  const detailedPreset = chatDisplayPrefs.slice(
    chatDisplayPrefs.indexOf("detailed: {"),
    chatDisplayPrefs.indexOf("function defaultPrefs"),
  );

  assert.match(normalPreset, /showMcp:\s*true/);
  assert.match(normalPreset, /showHooks:\s*false/);
  assert.match(normalPreset, /processDefaultOpen:\s*false/);
  assert.match(detailedPreset, /processDefaultOpen:\s*true/);
});

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
  assert.match(preferencesCss, /container-type:\s*inline-size;/);
  assert.doesNotMatch(terminalCss, /max-width:\s*920px/);
  assert.doesNotMatch(browserCss, /max-width:\s*760px/);
});

test("preference-backed tabs render independent glass cards", () => {
  assert.match(
    settingsCardCss,
    /\.prefs-page\.is-embedded \.prefs-category-stack\s*\{[\s\S]*?gap:\s*14px;[\s\S]*?background:\s*transparent;/,
  );
  assert.match(
    settingsMaterialCss,
    /> :is\(\.prefs-card, \.prefs-section\)\s*\{[\s\S]*?border:\s*var\(--settings-panel-border-width, 0\.5px\) solid[\s\S]*?border-radius:\s*var\(--settings-panel-radius,[\s\S]*?background:\s*var\(--settings-panel-background/,
  );
  assert.match(
    preferencesCss,
    /\.prefs-page\.is-embedded \.prefs-category-stack--appearance\s*\{[\s\S]*?background:\s*transparent;[\s\S]*?box-shadow:\s*none;/,
  );
  assert.match(
    preferencesCss,
    /\.prefs-page\.is-embedded \.prefs-category-stack--appearance > \.prefs-card\s*\{[\s\S]*?border:\s*var\(--settings-panel-border-width, 0\.5px\) solid[\s\S]*?background:\s*var\(--settings-panel-background[\s\S]*?backdrop-filter:\s*var\(--settings-panel-backdrop/,
  );
});

test("appearance and conversation use responsive grouped layouts", () => {
  assert.match(preferences, /prefs-category-stack--appearance/);
  assert.match(preferences, /prefs-card--appearance-material/);
  assert.match(preferences, /prefs-card--appearance-motion/);
  assert.match(preferences, /appearance-control-row/);
  assert.match(preferences, /prefs\.appearance\.material\.title/);
  assert.match(preferences, /colorControls=\{/);
  assert.match(preferences, /prefs\.appearance\.motion\.title/);
  assert.doesNotMatch(preferences, /prefs-appearance-preview/);
  assert.match(preferences, /prefs-card--conversation-display/);
  assert.match(preferences, /prefs-card--conversation-controls/);
  assert.match(preferences, /prefs-conversation-choice-column/);
  assert.match(preferences, /ConversationLayoutPreview/);
  assert.match(preferences, /prefs-conversation-preview/);
  assert.match(preferences, /data-layout=\{prefs\.answerLayout\}/);
  assert.match(preferences, /prefs\.verbosity/);
  assert.match(
    preferencesCss,
    /\.prefs-category-stack--appearance:not\(\[hidden\]\)[\s\S]*?grid-template-columns:\s*repeat\(2,/,
  );
  assert.match(
    preferencesCss,
    /\.prefs-category-stack--appearance > \.prefs-card\s*\{[\s\S]*?grid-column:\s*1 \/ -1/,
  );
  assert.match(
    preferencesCss,
    /\.app-icon-grid\s*\{[\s\S]*?grid-template-columns:\s*repeat\(5,/,
  );

  const themeOptions = preferences.slice(
    preferences.indexOf("const themeOptions"),
    preferences.indexOf("const colorStyleOptions"),
  );
  assert.ok(
    themeOptions.indexOf('id: "light"') < themeOptions.indexOf('id: "auto"'),
  );
  assert.ok(
    themeOptions.indexOf('id: "auto"') < themeOptions.indexOf('id: "dark"'),
  );

  const colorOptions = preferences.slice(
    preferences.indexOf("const colorStyleOptions"),
    preferences.indexOf("const langOptions"),
  );
  assert.ok(
    colorOptions.indexOf('id: "unified"') <
      colorOptions.indexOf('id: "dynamic"'),
  );
  assert.ok(
    colorOptions.indexOf('id: "dynamic"') <
      colorOptions.indexOf('id: "colorful"'),
  );

  assert.match(preferences, /min=\{GLASS_INTENSITY_MIN\}/);
  assert.match(preferences, /max=\{GLASS_INTENSITY_MAX\}/);
  assert.match(
    preferencesCss,
    /\.prefs-card--conversation-controls \.prefs-toggle-list[\s\S]*?grid-template-columns:\s*repeat\(2,/,
  );
  assert.match(
    preferencesCss,
    /\.prefs-conversation-layout-grid\s*\{[\s\S]*?grid-template-columns:\s*minmax\(250px, 0\.78fr\) minmax\(320px, 1\.22fr\);/,
  );

  const conversation = preferences.slice(
    preferences.indexOf('hidden={activeCategory !== "conversation"}'),
    preferences.indexOf('hidden={activeCategory !== "context"}'),
  );
  const layoutCard = conversation.slice(
    conversation.indexOf("prefs-card--conversation-display"),
    conversation.indexOf("prefs-card--conversation-controls"),
  );
  const controlsCard = conversation.slice(
    conversation.indexOf("prefs-card--conversation-controls"),
  );
  assert.match(layoutCard, /prefs-conversation-verbosity-options/);
  assert.ok(
    layoutCard.indexOf("prefs-conversation-choice-column") <
      layoutCard.indexOf("<ConversationLayoutPreview"),
  );
  assert.doesNotMatch(controlsCard, /prefs-conversation-verbosity-options/);
  assert.doesNotMatch(preferencesCss, /prefs-conversation-controls-grid/);
  assert.match(prototypePages, /conversation-layout-composer/);
  assert.ok(
    prototypePages.indexOf("conversation-choice-stack") <
      prototypePages.indexOf("conversation-live-preview"),
  );
  assert.match(
    prototypeCss,
    /\.conversation-layout-composer\s*\{[^}]*grid-template-columns:\s*minmax\(260px, 0\.78fr\) minmax\(340px, 1\.22fr\);/,
  );
  const general = preferences.slice(
    preferences.indexOf('hidden={activeCategory !== "general"}'),
    preferences.indexOf('hidden={activeCategory !== "about"}'),
  );
  assert.doesNotMatch(conversation, /<SidebarVisibleSetting/);
  assert.match(general, /<SidebarVisibleSetting/);
});

test("general settings use direct peer cards without a legacy wrapper", () => {
  const general = preferences.slice(
    preferences.indexOf('hidden={activeCategory !== "general"}'),
    preferences.indexOf('hidden={activeCategory !== "about"}'),
  );
  assert.match(general, /prefs-card--language/);
  assert.match(general, /prefs-card--system/);
  assert.doesNotMatch(
    general,
    /prefs-card--general(?:"|\s)|prefs-general-group|prefs-general-divider/,
  );
});

test("terminal settings use a full-width mode card and balanced detail columns", () => {
  assert.match(
    terminal,
    /className="prefs-category-stack terminal-settings-layout"/,
  );
  assert.match(terminal, /terminal-settings-section--mode/);
  assert.match(terminal, /terminal-settings-section--font/);
  assert.match(terminal, /terminal-settings-section--behavior/);
  assert.match(terminal, /terminal-font-preview--embedded/);
  assert.match(terminal, /terminal-settings-range/);
  assert.match(terminal, /terminal-cursor-options/);
  assert.match(terminal, /terminal-reset-button/);
  assert.match(terminal, /scrollbackPresets\.includes\(settings\.scrollback\)/);
  assert.doesNotMatch(terminal, /terminal-settings-preview-pane/);
  assert.match(
    terminalCss,
    /\.terminal-settings-layout:not\(\[hidden\]\)[\s\S]*?grid-template-columns:\s*repeat\(2, minmax\(0, 1fr\)\);/,
  );
  assert.match(
    terminalCss,
    /\.terminal-settings-section--mode[\s\S]*?grid-column:\s*1 \/ -1;/,
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
  const diagnosticsRuntime = `${diagnosticsHook}\n${diagnosticsModel}`;
  assert.match(preferences, /prefs-category-stack--diagnostics/);
  assert.match(preferences, /prefs-card--diagnostics/);
  assert.match(preferences, /prefs-diag-status-grid/);
  assert.match(preferences, /DiagnosticStatusCard/);
  assert.match(preferences, /prefs-diag-filter-grid/);
  assert.match(preferences, /prefs-diag-search/);
  assert.match(preferences, /prefs-diag-live/);
  assert.match(diagnosticsRuntime, /setInterval\(/);
  assert.match(preferences, /DiagnosticLogTimeRange/);
  assert.match(diagnosticsRuntime, /sinceMs/);
  assert.match(diagnosticsRuntime, /untilMs/);
  assert.match(preferences, /datetime-local/);
  assert.match(preferences, /visibleLogRows/);
  assert.match(diagnosticsRuntime, /diagnosticLogLevel/);
  assert.match(preferences, /prefs-diag-log-list/);
  assert.match(preferences, /const LINE_PRESETS = \[50, 100, 200, 500\]/);
  assert.match(preferences, /logsCopied/);
  assert.match(
    diagnosticsRuntime,
    /invoke<DiagnosticsStatusDto>\("get_diagnostics_status"\)/,
  );
  assert.match(
    diagnosticsRuntime,
    /invoke<string \| null>\("export_diagnostics_bundle"\)/,
  );
  assert.match(preferences, /prefs-diag-export-card/);
  assert.match(preferences, /prefs-category-stack--about/);
  assert.match(preferences, /prefs-card--about-hero/);
  assert.match(preferences, /prefs-card--about-update/);
  assert.match(preferences, /prefs-card--about-project/);
  assert.match(preferences, /prefs-about-badges/);
  assert.match(preferences, /getIdentifier/);
  assert.match(preferences, /getTauriVersion/);
  assert.match(preferences, /prefs-about-setting-list/);
  assert.match(preferences, /updatesUnavailable/);
  assert.match(preferences, /licenseValue/);
  assert.match(preferences, /invoke<AppUpdateInfo>\("check_app_update"\)/);
  assert.match(preferences, /invoke\("install_app_update"\)/);
  assert.match(updater, /\.download_and_install\(/);
  assert.match(updater, /app\.restart\(\)/);
  assert.doesNotMatch(preferences, /ABOUT_FEATURES\.map/);
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
    /\.prefs-diag-status-grid\s*\{[\s\S]*?grid-template-columns:\s*repeat\(4,/,
  );
  assert.match(preferencesCss, /\.prefs-diag-export-card\s*\{/);
  assert.match(preferencesCss, /\.prefs-diag-log-row\.is-error/);
  assert.match(preferencesCss, /\.prefs-diag-live\[data-active="true"\]/);
  assert.match(preferencesCss, /\.prefs-diag-log-meta time/);
  assert.match(
    preferencesCss,
    /\.prefs-category-stack--about:not\(\[hidden\]\)[\s\S]*?grid-template-columns:\s*repeat\(2,[\s\S]*?width:\s*min\(100%, 980px\);/,
  );
  assert.match(
    preferencesCss,
    /\.prefs-card--about-hero\s*\{[\s\S]*?grid-column:\s*1 \/ -1;/,
  );
});
