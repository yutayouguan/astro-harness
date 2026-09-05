import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const [app, tabs, panel, dock, css, tauriCommands, tauriLib] =
  await Promise.all(
    [
      "../../App.tsx",
      "./settingsTabs.ts",
      "../../components/settings/BrowserSettingsPanel.tsx",
      "../../components/chat/BrowserDock.tsx",
      "../../styles/features/browser-settings.css",
      "../../../src-tauri/src/commands/browser.rs",
      "../../../src-tauri/src/lib.rs",
    ].map((path) => readFile(new URL(path, import.meta.url), "utf8")),
  );

test("browser settings has a dedicated settings route and panel", () => {
  assert.match(
    tabs,
    /id:\s*"browser",\s*labelKey:\s*"settings\.sidebar\.tab\.browser"/,
  );
  assert.match(app, /settingsTab === "browser"/);
  assert.match(app, /<BrowserSettingsPanel/);
});

test("browser settings are backed by Tauri commands and runtime controls", () => {
  for (const command of [
    "browser_get_settings",
    "browser_set_settings",
    "browser_revoke_approval",
  ]) {
    assert.match(panel, new RegExp(`"${command}"`));
    assert.match(tauriCommands, new RegExp(`fn ${command}`));
    assert.match(tauriLib, new RegExp(`commands::browser::${command}`));
  }
  assert.match(panel, /allowLoopback/);
  assert.match(panel, /downloadsEnabled/);
  assert.match(panel, /approvalRules/);
});

test("new tabs defer to the configured home page", () => {
  assert.match(dock, /run\("new_tab"\)/);
  assert.doesNotMatch(dock, /preview\?\.url \|\| "https:\/\/example\.com"/);
});

test("browser settings preserve light dark and narrow layouts", () => {
  assert.match(css, /html\[data-theme="dark"\]/);
  assert.match(css, /@container \(max-width: 620px\)/);
  assert.match(css, /@media \(prefers-reduced-motion: reduce\)/);
  assert.doesNotMatch(css, /transition:\s*all/);
});
