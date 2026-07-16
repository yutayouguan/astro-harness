import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

test("chat message anchors share the bottom button alignment axis", async () => {
  const css = await readFile(
    new URL("../../styles/features/chat/navigation.css", import.meta.url),
    "utf8",
  );
  const trackRule = css.match(/\.chat-msg-nav-track\s*\{(?<body>[\s\S]*?)\}/);

  assert.ok(trackRule?.groups?.body, "missing chat message navigation track rule");
  assert.match(
    trackRule.groups.body,
    /padding:\s*10px 0 10px 28px;/,
    "the track must not inset message anchors from the bottom button axis",
  );
});

test("chat anchor labels use a translucent blurred surface", async () => {
  const css = await readFile(
    new URL("../../styles/features/chat/navigation.css", import.meta.url),
    "utf8",
  );
  const labelRule = css.match(/\.chat-msg-nav-label\s*\{(?<body>[\s\S]*?)\}/);

  assert.ok(labelRule?.groups?.body, "missing chat message navigation label rule");
  assert.match(labelRule.groups.body, /background:[\s\S]*var\(--nav-label-bg\)/);
  assert.match(labelRule.groups.body, /backdrop-filter:\s*blur\(/);
  assert.match(labelRule.groups.body, /-webkit-backdrop-filter:\s*blur\(/);
});
