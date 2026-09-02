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
  assert.match(app, /const terminalDockPresence = useDeferredPresence/);
  assert.match(app, /<SquareTerminal/);
  assert.match(app, /<TerminalDock/);
  assert.match(app, /open=\{terminalDockPresence\.visible\}/);
  assert.doesNotMatch(rightDock, /terminal/);
  assert.match(css, /\.terminal-dock\s*\{[\s\S]*?flex:\s*0 0 0;/);
  assert.match(
    css,
    /\.terminal-dock\.is-open\s*\{[\s\S]*?flex-basis:\s*var\(--terminal-dock-height\);/,
  );
  assert.match(css, /flex-basis 300ms cubic-bezier\(0\.22, 1, 0\.36, 1\)/);
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
  assert.match(component, /WRITE_CHUNK_BYTES/);
  assert.match(component, /encoded\.subarray/);
  assert.match(component, /restartGeneration/);
  assert.match(component, /if \(!open \|\| !host \|\| !projectRoot\) return;/);
  assert.match(component, /restartAfterExitRef/);
  assert.match(component, /<RefreshCw size=\{14\}/);
  assert.doesNotMatch(component, /<Square size=/);
});

test("terminal dock resets ownership when the active project changes", () => {
  const app = source("App.tsx");
  const component = source("components/chat/TerminalDock.tsx");

  assert.match(app, /key=\{activeProject\.id\}/);
  assert.match(component, /setSession\(null\)/);
});

test("terminal dock configures the xterm 6 custom scrollbar instead of the legacy viewport", () => {
  const component = source("components/chat/TerminalDock.tsx");
  const css = source("styles/features/chat/terminal-dock.css");

  assert.match(component, /SCROLLBAR_WIDTH = 4/);
  assert.match(component, /overviewRuler:\s*\{ width: SCROLLBAR_WIDTH \}/);
  assert.match(css, /\.xterm-scrollable-element/);
  assert.match(css, /> \.scrollbar/);
  assert.match(css, /> \.slider/);
  assert.doesNotMatch(css, /xterm-viewport::/);
});
