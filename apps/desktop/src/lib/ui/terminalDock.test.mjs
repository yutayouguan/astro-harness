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

test("terminal content is borderless and keeps all four xterm corners square", () => {
  const css = source("styles/features/chat/terminal-dock.css");
  const outerDock = css.match(/\.terminal-dock\s*\{([\s\S]*?)\}/)?.[1] ?? "";

  assert.match(
    css,
    /\.terminal-dock-screen \.xterm\s*\{[\s\S]*?overflow:\s*hidden;[\s\S]*?border:\s*0;[\s\S]*?border-radius:\s*0;[\s\S]*?box-shadow:\s*none;/,
  );
  assert.doesNotMatch(outerDock, /border-radius/);
  assert.match(
    css,
    /\.chat-layout-with-right:has\(\.terminal-dock\.is-open\)\s*\{[\s\S]*?border-radius:\s*28px 28px 0 0;/,
  );
});

test("terminal always uses the denser readable glass material", () => {
  const app = source("App.tsx");
  const dock = source("components/chat/TerminalTabsDock.tsx");
  const css = source("styles/features/chat/terminal-dock.css");

  assert.match(dock, /"--terminal-screen-bg"/);
  assert.match(dock, /allowTransparency:\s*true/);
  assert.doesNotMatch(app, /wallpaperActive=\{wallpaperEnabled\}/);
  assert.doesNotMatch(dock, /wallpaperActive/);
  assert.doesNotMatch(dock, /"data-wallpaper"/);
  assert.match(css, /html\[data-theme="light"\]/);
  assert.match(css, /--terminal-screen-bg:\s*rgba\(0, 0, 0, 0\)/);
  assert.match(css, /padding:\s*4px 10px 0 14px/);
  assert.match(css, /border-bottom-right-radius:\s*0/);
  assert.match(css, /border-bottom-left-radius:\s*0/);
  assert.match(css, /\.terminal-dock-screen \.xterm-screen canvas/);
  assert.match(
    css,
    /background-color:\s*var\(--terminal-screen-bg\) !important/,
  );
  assert.match(css, /\.terminal-dock\s*\{[\s\S]*?backdrop-filter:\s*blur\(/);
  assert.match(css, /backdrop-filter:\s*blur\(/);
  assert.match(css, /prefers-reduced-transparency/);
  assert.match(css, /prefers-contrast:\s*more/);
});

test("terminal opens and closes as one bottom drawer without competing size animations", () => {
  const css = source("styles/features/chat/terminal-dock.css");
  const closed = css.match(/\.terminal-dock\s*\{([\s\S]*?)\}/)?.[1] ?? "";
  const opened =
    css.match(/\.terminal-dock\.is-open\s*\{([\s\S]*?)\}/)?.[1] ?? "";

  assert.match(closed, /flex:\s*0 0 0/);
  assert.match(closed, /transform:\s*translateY\(44px\)/);
  assert.match(closed, /flex-basis 300ms cubic-bezier\(0\.22, 1, 0\.36, 1\)/);
  assert.doesNotMatch(closed, /scale\(/);
  assert.doesNotMatch(closed, /height 300ms|min-height 300ms|max-height 300ms/);
  assert.match(opened, /flex-basis:\s*var\(--terminal-dock-height\)/);
  assert.match(opened, /transform:\s*translateY\(0\)/);
});
