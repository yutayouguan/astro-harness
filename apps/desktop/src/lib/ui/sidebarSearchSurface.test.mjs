import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const sidebarPolishStyles = await readFile(
  new URL("../../styles/features/shell/layout/sidebar-polish.css", import.meta.url),
  "utf8",
);

function rule(css, selector) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return css.match(
    new RegExp(`(?:^|\\n)${escaped}\\s*\\{(?<body>[\\s\\S]*?)\\}`),
  )?.groups?.body;
}

test("sidebar search trigger uses the same quiet navigation surface", () => {
  const trigger = rule(
    sidebarPolishStyles,
    ".sidebar-global-search .expandable-search-btn",
  );

  assert.ok(trigger, "missing sidebar search trigger styles");
  assert.match(trigger, /background:\s*transparent;/);
  assert.match(trigger, /box-shadow:\s*none;/);
  assert.match(trigger, /backdrop-filter:\s*none;/);
  assert.match(trigger, /border-radius:\s*10px;/);
});

test("expanded sidebar search uses a matte focused treatment", () => {
  const field = rule(
    sidebarPolishStyles,
    ".sidebar-global-search .expandable-search-field",
  );
  const focus = rule(
    sidebarPolishStyles,
    ".sidebar-global-search .expandable-search-field:focus-within",
  );

  assert.ok(field, "missing expanded sidebar search styles");
  assert.match(field, /background:\s*color-mix\(in srgb, var\(--ink\) 4%, transparent\);/);
  assert.match(field, /box-shadow:\s*none;/);
  assert.match(field, /backdrop-filter:\s*none;/);
  assert.ok(focus, "missing expanded sidebar search focus treatment");
  assert.match(focus, /border-color:\s*color-mix/);
  assert.match(focus, /box-shadow:\s*none;/);
});

test("dark sidebar search has a scoped low-white override", () => {
  const dark = rule(
    sidebarPolishStyles,
    'html[data-theme="dark"] .sidebar-global-search .expandable-search-btn,\nhtml[data-theme="dark"] .sidebar-global-search .expandable-search-field',
  );

  assert.ok(dark, "missing dark sidebar search styles");
  assert.match(dark, /color-mix\(in srgb, #fff 4%, transparent\)/);
  assert.match(dark, /box-shadow:\s*none;/);
  assert.doesNotMatch(dark, /backdrop-filter:\s*blur/);
});

test("collapsed and expanded sidebar search keep the same row height", () => {
  const trigger = rule(
    sidebarPolishStyles,
    ".sidebar-global-search .expandable-search-btn",
  );
  const field = rule(
    sidebarPolishStyles,
    ".sidebar-global-search .expandable-search-field",
  );

  assert.ok(trigger, "missing polished sidebar search trigger styles");
  assert.ok(field, "missing polished expanded sidebar search styles");
  assert.match(trigger, /height:\s*40px;/);
  assert.match(field, /height:\s*40px;/);
});
