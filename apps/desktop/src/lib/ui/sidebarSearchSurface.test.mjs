import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const projectStyles = await readFile(
  new URL("../../styles/features/shell/layout/projects.css", import.meta.url),
  "utf8",
);

function rule(css, selector) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return css.match(
    new RegExp(`(?:^|\\n)${escaped}\\s*\\{(?<body>[\\s\\S]*?)\\}`),
  )?.groups?.body;
}

test("sidebar search trigger uses a restrained tinted glass surface", () => {
  const trigger = rule(
    projectStyles,
    ".sidebar-global-search .expandable-search-btn",
  );

  assert.ok(trigger, "missing sidebar search trigger styles");
  assert.match(trigger, /var\(--tone-soft/);
  assert.match(trigger, /backdrop-filter:\s*blur/);
  assert.match(trigger, /inset 0 1px 0 rgba\(255, 255, 255, 0\.18\)/);
  assert.doesNotMatch(trigger, /var\(--glass\) 88%/);
});

test("expanded sidebar search has a focused tinted treatment", () => {
  const field = rule(
    projectStyles,
    ".sidebar-global-search .expandable-search-field",
  );
  const focus = rule(
    projectStyles,
    ".sidebar-global-search .expandable-search-field:focus-within",
  );

  assert.ok(field, "missing expanded sidebar search styles");
  assert.match(field, /var\(--tone-soft/);
  assert.match(field, /backdrop-filter:\s*blur/);
  assert.ok(focus, "missing expanded sidebar search focus treatment");
  assert.match(focus, /border-color:\s*color-mix/);
  assert.match(focus, /inset 0 0 0 1px/);
});

test("dark sidebar search has a scoped low-white override", () => {
  const dark = rule(
    projectStyles,
    'html[data-theme="dark"] .sidebar-global-search .expandable-search-btn,\nhtml[data-theme="dark"] .sidebar-global-search .expandable-search-field',
  );

  assert.ok(dark, "missing dark sidebar search styles");
  assert.match(dark, /rgba\(255, 255, 255, 0\.05\)/);
  assert.match(dark, /rgba\(8, 6, 18, 0\.48\)/);
  assert.doesNotMatch(dark, /rgba\(255, 255, 255, 0\.3\)/);
});
