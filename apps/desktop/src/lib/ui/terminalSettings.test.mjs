import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const [app, tabs, panel, dock, settings, css, proto, server] =
  await Promise.all(
    [
      "../../App.tsx",
      "./settingsTabs.ts",
      "../../components/settings/TerminalSettingsPanel.tsx",
      "../../components/chat/TerminalDock.tsx",
      "../terminal/terminalSettings.ts",
      "../../styles/features/terminal-settings.css",
      "../../../../../crates/agent-proto/proto/astro.proto",
      "../../../../../crates/agent-server/src/grpc/astro_service.rs",
    ].map((path) => readFile(new URL(path, import.meta.url), "utf8")),
  );

test("terminal settings has a dedicated route and panel", () => {
  assert.match(
    tabs,
    /id:\s*"terminal",\s*labelKey:\s*"settings\.sidebar\.tab\.terminal"/,
  );
  assert.match(app, /settingsTab === "terminal"/);
  assert.match(app, /<TerminalSettingsPanel/);
});

test("terminal settings provide Nerd Font defaults and bounded display controls", () => {
  assert.match(settings, /MesloLGS NF/);
  assert.match(settings, /fontSize: finiteNumber/);
  assert.match(settings, /scrollback: Math\.round/);
  assert.match(panel, /terminal-font-preview/);
  assert.match(dock, /subscribeTerminalSettings/);
  assert.match(dock, /terminal\.options\.fontFamily/);
});

test("terminal execution mode is explicit across the desktop boundary", () => {
  assert.match(proto, /string execution_mode = 5/);
  assert.match(proto, /bool replace_mode_mismatch = 6/);
  assert.match(dock, /executionMode: settings\.executionMode/);
  assert.match(server, /"system" => sandbox::SandboxPolicy::new/);
  assert.match(server, /replace_mode_mismatch/);
});

test("terminal settings preserve dark, narrow, and reduced-motion layouts", () => {
  assert.match(css, /html\[data-theme="dark"\]/);
  assert.match(css, /@media \(max-width: 680px\)/);
  assert.match(css, /@media \(prefers-reduced-motion: reduce\)/);
  assert.doesNotMatch(css, /transition:\s*all/);
});
