import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const cssUrl = new URL(
  "../../styles/features/chat/answer-panel.css",
  import.meta.url,
);
const coreCssUrl = new URL(
  "../../styles/features/chat/core.css",
  import.meta.url,
);

test("user and assistant entries share the same outer card surface", async () => {
  const css = await readFile(cssUrl, "utf8");

  assert.match(
    css,
    /html\[data-theme\] \.bubble:is\(\.assistant, \.user\) \{[\s\S]*?border-radius: var\(--answer-radius\);[\s\S]*?background: var\(--glass-layer, var\(--glass-panel\)\);[\s\S]*?box-shadow: var\(--shadow-card\), var\(--glass-rim\);/,
  );
  assert.match(
    css,
    /html\[data-theme="light"\] \.bubble:is\(\.assistant, \.user\)/,
  );
  assert.match(
    css,
    /html\[data-theme="dark"\] \.bubble:is\(\.assistant, \.user\)/,
  );
});

test("assistant avatars reuse the answer card glass material", async () => {
  const css = await readFile(coreCssUrl, "utf8");
  const avatar = css.match(/\.avatar\s*\{(?<body>[\s\S]*?)\n\}/)?.groups?.body;

  assert.ok(avatar, "missing assistant avatar styles");
  for (const declaration of [
    "background: var(--glass-layer, var(--glass-panel));",
    "border: 0.5px solid var(--glass-edge);",
    "box-shadow: var(--shadow-card), var(--glass-rim);",
    "backdrop-filter: var(--backdrop-glass);",
  ]) {
    assert.ok(avatar.includes(declaration), `avatar is missing ${declaration}`);
  }
  assert.doesNotMatch(css, /html\[data-theme="dark"\] \.avatar\s*\{/);
});
