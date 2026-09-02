import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const [app, dock, tabs, css, proto, manager, tauri] = await Promise.all(
  [
    "../../App.tsx",
    "../../components/chat/TerminalTabsDock.tsx",
    "../terminal/terminalTabs.ts",
    "../../styles/features/chat/terminal-dock.css",
    "../../../../../crates/agent-proto/proto/astro.proto",
    "../../../../../crates/agent-tools/src/terminal_session.rs",
    "../../../src-tauri/src/commands/terminal.rs",
  ].map((path) => readFile(new URL(path, import.meta.url), "utf8")),
);

test("terminal dock routes to the multi-tab implementation", () => {
  assert.match(app, /import\("\.\/components\/chat\/TerminalTabsDock"\)/);
  assert.match(dock, /role="tablist"/);
  assert.match(dock, /createTerminalClientId/);
  assert.match(dock, /MAX_TERMINAL_TABS/);
  assert.match(css, /\.terminal-tab\.is-active/);
});

test("new projects start with separate user and AI terminals", () => {
  assert.match(tabs, /executionMode: userExecutionMode/);
  assert.match(tabs, /executionMode: "project" as const/);
  assert.match(tabs, /agentDefault: true/);
  assert.match(dock, /clientToken: tab\.clientId/);
  assert.match(dock, /agentDefault: tab\.agentDefault/);
});

test("only the active tab is started eagerly", () => {
  assert.match(dock, /if \(activeTab && !activeSession && !activeError\) void openTab\(activeTab\)/);
  assert.doesNotMatch(dock, /for \(const tab of tabs\)/);
});

test("desktop tab tokens are idempotent and closing is session-scoped", () => {
  assert.match(proto, /string client_token = 7/);
  assert.match(proto, /rpc CloseTerminal/);
  assert.match(manager, /desktop_by_token/);
  assert.match(manager, /MAX_TERMINALS_PER_SCOPE: usize = 8/);
  assert.match(tauri, /fn terminal_close/);
  assert.match(dock, /"terminal_close"/);
});

test("tab layout and keyboard operations are persisted locally", () => {
  assert.match(tabs, /astro\.terminal\.tabs\.v1\./);
  assert.match(dock, /saveTerminalTabLayout/);
  assert.match(dock, /key === "t"/);
  assert.match(dock, /key === "w"/);
  assert.match(dock, /event\.key === "\[" \|\| event\.key === "\]"/);
});
