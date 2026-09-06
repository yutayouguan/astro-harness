import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const headerStyles = await readFile(
  new URL(
    "../../styles/features/shell/layout/content-header.css",
    import.meta.url,
  ),
  "utf8",
);
const navigationStyles = await readFile(
  new URL("../../styles/features/shell/layout/navigation.css", import.meta.url),
  "utf8",
);

test("page title icons remain outline-only", () => {
  const titleIconRule = headerStyles.match(
    /\.page-title-icon svg\s*\{(?<body>[\s\S]*?)\n\}/,
  )?.groups?.body;

  assert.ok(titleIconRule, "missing page title icon rule");
  assert.match(titleIconRule, /fill:\s*none;/);
  assert.doesNotMatch(titleIconRule, /fill:\s*var\(--nav-grad-paint\)/);
  assert.match(
    headerStyles,
    /\.page-title-icon svg \.nav-icon-fill\s*\{\s*fill:\s*none;/,
  );
});

test("sidebar active icons retain their separate filled state", () => {
  assert.match(
    navigationStyles,
    /\.nav-item\.active \.nav-icon svg,[\s\S]*?fill:\s*var\(--nav-grad-paint\);/,
  );
});
