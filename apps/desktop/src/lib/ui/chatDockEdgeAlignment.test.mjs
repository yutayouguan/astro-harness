import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const headerStyles = await readFile(
  new URL("../../styles/features/shell/header.css", import.meta.url),
  "utf8",
);

test("full-canvas browser and terminal surfaces align with the pinned sidebar", () => {
  assert.match(
    headerStyles,
    /\.page-body\.page-body--chat:has\(\s*> \.chat-layout-with-right\.is-browser-expanded\s*\)/,
  );
  assert.match(
    headerStyles,
    /\.page-body\.page-body--chat:has\(\s*> \.chat-layout-with-right > \.chat-main > \.terminal-dock\.is-open\s*\)/,
  );
  assert.match(
    headerStyles,
    /\.terminal-dock\.is-open\s*\)\s*\{\s*padding-left:\s*0;/,
  );
});

test("the dock edge inset follows the drawer motion and reduced-motion preference", () => {
  assert.match(
    headerStyles,
    /\.content-pane > \.page-body\.page-body--chat\s*\{[\s\S]*?transition:\s*padding-left 300ms cubic-bezier\(0\.22, 1, 0\.36, 1\);/,
  );
  assert.match(
    headerStyles,
    /@media \(prefers-reduced-motion: reduce\)[\s\S]*?\.content-pane > \.page-body\.page-body--chat\s*\{\s*transition:\s*none;/,
  );
});
