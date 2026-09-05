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
  assert.match(app, /persistAfterOpen:\s*true/);
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
  assert.match(
    css,
    /\.terminal-dock\.is-resizing\s*\{[\s\S]*?transition:\s*none/,
  );
  assert.match(app, /<TerminalDock/);
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

test("terminal dock stays mounted after its first open so later toggles restore it", () => {
  const presence = source("hooks/ui/useDeferredPresence.ts");
  const dock = source("components/chat/TerminalTabsDock.tsx");

  assert.match(presence, /persistAfterOpen\?: boolean/);
  assert.match(presence, /if \(!persistAfterOpen\)/);
  assert.match(presence, /setMounted\(false\)/);
  assert.match(
    dock,
    /if \(activeTab && !activeSession && !activeError\) void openTab\(activeTab\)/,
  );
  assert.match(dock, /visible=\{open\}/);
});

test("terminal height dragging paints once per frame and commits state on release", () => {
  const dock = source("components/chat/TerminalTabsDock.tsx");

  assert.match(dock, /requestAnimationFrame\(paintHeight\)/);
  assert.match(dock, /style\.setProperty\(\s*"--terminal-dock-height"/);
  assert.match(dock, /setHeight\(finalHeight\)/);
  const moveStart = dock.indexOf("const move =");
  const stopStart = dock.indexOf("const stopListening", moveStart);
  assert.ok(moveStart >= 0 && stopStart > moveStart);
  assert.doesNotMatch(dock.slice(moveStart, stopStart), /setHeight\(/);
  assert.match(dock, /role="separator"/);
  assert.match(dock, /aria-valuenow=\{Math\.round\(height\)\}/);
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

test("terminal content clips all four xterm corners to the inner radius", () => {
  const css = source("styles/features/chat/terminal-dock.css");
  const outerDock = css.match(/\.terminal-dock\s*\{([\s\S]*?)\}/)?.[1] ?? "";

  assert.match(
    css,
    /\.terminal-dock-screen \.xterm\s*\{[\s\S]*?overflow:\s*hidden;[\s\S]*?border-radius:\s*12px;/,
  );
  assert.doesNotMatch(outerDock, /border-radius/);
});

test("terminal uses a denser readable material when wallpaper is active", () => {
  const app = source("App.tsx");
  const dock = source("components/chat/TerminalTabsDock.tsx");
  const css = source("styles/features/chat/terminal-dock.css");

  assert.match(app, /root\.setAttribute\("data-wallpaper", "true"\)/);
  assert.match(app, /root\.removeAttribute\("data-wallpaper"\)/);
  assert.match(dock, /"--terminal-screen-bg"/);
  assert.match(dock, /"data-wallpaper"/);
  assert.match(css, /html\[data-wallpaper="true"\]\[data-theme="light"\]/);
  assert.match(css, /--terminal-screen-bg:\s*rgb\(246, 248, 252\)/);
  assert.match(css, /html\[data-wallpaper="true"\] \.terminal-dock\s*\{/);
  assert.match(css, /backdrop-filter:\s*blur\(/);
  assert.match(css, /prefers-reduced-transparency/);
  assert.match(css, /prefers-contrast:\s*more/);
});
