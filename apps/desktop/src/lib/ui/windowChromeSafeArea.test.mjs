import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const appSource = await readFile(
  new URL("../../App.tsx", import.meta.url),
  "utf8",
);
const primitives = await readFile(
  new URL("../../styles/tokens/primitive.css", import.meta.url),
  "utf8",
);
const headerStyles = await readFile(
  new URL("../../styles/features/shell/header.css", import.meta.url),
  "utf8",
);
const shellStyles = await readFile(
  new URL("../../styles/features/shell/shell.css", import.meta.url),
  "utf8",
);

test("the app shell exposes explicit sidebar occupancy", () => {
  assert.match(
    appSource,
    /data-sidebar-state=\{sidebar\.sidebarVisible \|\| interfaceTourActive \? "visible" : "collapsed"\}/,
  );
});

test("collapsed window chrome uses one shared safe-left token", () => {
  assert.match(primitives, /--titlebar-traffic-w:\s*78px;/);
  assert.match(primitives, /--titlebar-sidebar-toggle-w:\s*34px;/);
  assert.match(
    primitives,
    /--window-chrome-safe-left:\s*calc\([\s\S]*?var\(--titlebar-traffic-w\)[\s\S]*?var\(--titlebar-sidebar-toggle-w\)[\s\S]*?\);/,
  );

  assert.match(
    headerStyles,
    /\.app-shell\[data-sidebar-state="collapsed"\] \.content-header\s*\{[\s\S]*?padding-inline-start:\s*var\(--window-chrome-safe-left\);/,
  );
  assert.match(
    headerStyles,
    /\.app-shell\[data-sidebar-state="collapsed"\][\s\S]*?\.feature-content-inline[\s\S]*?:is\(\s*\.cron-toolbar,\s*\.loop-toolbar,\s*\.loop-editor-toolbar,\s*\.plugins-command-bar\s*\)\s*\{[\s\S]*?padding-inline-start:\s*max\([\s\S]*?var\(--window-chrome-safe-left\)[\s\S]*?var\(--page-body-inline-padding\)/,
  );
});

test("window chrome uses semantic elevation tokens instead of ad-hoc layers", () => {
  assert.match(primitives, /--z-wallpaper:\s*0;/);
  assert.match(primitives, /--z-window-drag:\s*40;/);
  assert.match(primitives, /--z-titlebar:\s*42;/);
  assert.match(primitives, /--z-window-controls:\s*45;/);
  assert.match(primitives, /--z-overlay-base:\s*1200;/);
  assert.match(shellStyles, /z-index:\s*var\(--z-window-drag\)/);
  assert.match(shellStyles, /z-index:\s*var\(--z-window-controls\)/);
  assert.match(headerStyles, /z-index:\s*var\(--z-titlebar\)/);
  assert.match(headerStyles, /z-index:\s*var\(--z-window-controls\)/);
});
