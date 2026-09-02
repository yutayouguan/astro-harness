import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const headerStyles = await readFile(
  new URL("../../styles/features/shell/header.css", import.meta.url),
  "utf8",
);
const primitives = await readFile(
  new URL("../../styles/tokens/primitive.css", import.meta.url),
  "utf8",
);
const shellStyles = await readFile(
  new URL("../../styles/features/shell/shell.css", import.meta.url),
  "utf8",
);

function rule(css, selector) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return css.match(
    new RegExp(`(?:^|\\n)${escaped}\\s*\\{(?<body>[\\s\\S]*?)\\}`),
  )?.groups?.body;
}

test("page bodies use the window edge without a shared 22px inset", () => {
  const pageBody = rule(headerStyles, ".page-body");
  const expandedPageBody = rule(
    headerStyles,
    ".app-shell.is-chat-expanded .page-body",
  );

  assert.ok(pageBody, "missing page body rule");
  assert.match(pageBody, /--page-body-inline-padding:\s*0px;/);
  assert.match(pageBody, /padding:\s*0;/);
  assert.doesNotMatch(pageBody, /22px/);
  assert.ok(expandedPageBody, "missing expanded page body rule");
  assert.match(expandedPageBody, /padding:\s*0;/);
});

test("browser focus tabs clear the native sidebar control", () => {
  assert.match(
    headerStyles,
    /\.chat-layout-with-right\.is-browser-expanded \.browser-dock-header\s*\{\s*left:\s*var\(--window-chrome-safe-left\);/,
  );
  assert.match(
    headerStyles,
    /\.sidebar\.is-pinned\.is-icons\)[\s\S]*?\.browser-dock-header\s*\{\s*left:\s*calc\(var\(--window-chrome-safe-left\) - var\(--sidebar-w\)\);/,
  );
  assert.match(
    headerStyles,
    /\.sidebar\.is-pinned\.is-labels\)[\s\S]*?\.browser-dock-header\s*\{\s*left:\s*16px;/,
  );
});

test("browser titlebar controls receive pointer input above the drag surface", () => {
  assert.match(
    shellStyles,
    /\.app-shell\.has-browser-surface > \.native-drag-region\s*\{\s*pointer-events:\s*none;/,
  );
});

test("native controls and chat titles share one compact titlebar row", () => {
  assert.match(primitives, /--titlebar-control-row-h:\s*34px;/);
  assert.match(
    headerStyles,
    /\.titlebar-sidebar-toggle\s*\{\s*top:\s*6px;/,
  );
  assert.match(
    headerStyles,
    /\.content-header--chat\s*\{[\s\S]*?min-height:\s*var\(--titlebar-control-row-h\);[\s\S]*?padding:\s*0 16px;[\s\S]*?padding-inline-start:\s*var\(--window-chrome-safe-left\);/,
  );
  assert.match(
    headerStyles,
    /\.content-pane--chat \.browser-dock-header\s*\{\s*height:\s*var\(--titlebar-control-row-h\);/,
  );
});
