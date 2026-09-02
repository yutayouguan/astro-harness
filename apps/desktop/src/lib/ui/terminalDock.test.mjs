import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const source = (path) =>
  readFileSync(new URL(`../../${path}`, import.meta.url), "utf8");

test("terminal dock is a bottom surface independent from the exclusive right dock", () => {
  const app = source("App.tsx");
  const rightDock = source("lib/ui/chatRightDock.ts");
  const css = source("styles/features/chat/terminal-dock.css");

  assert.match(app, /terminalDockOpen/);
  assert.match(app, /<SquareTerminal/);
  assert.match(app, /<TerminalDock/);
  assert.doesNotMatch(rightDock, /terminal/);
  assert.match(css, /flex:\s*0 0 var\(--terminal-dock-height\)/);
  assert.match(css, /cursor:\s*ns-resize/);
});

test("terminal dock uses one shared backend session for input, output, resize and kill", () => {
  const component = source("components/chat/TerminalDock.tsx");

  assert.match(component, /from "@xterm\/xterm"/);
  assert.match(component, /"terminal_open"/);
  assert.match(component, /"terminal_read"/);
  assert.match(component, /"terminal_write"/);
  assert.match(component, /"terminal_resize"/);
  assert.match(component, /"terminal_kill"/);
  assert.match(component, /"terminal_open_external"/);
});
