import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

test("chat turn anchors reserve a left-side hit area", async () => {
  const css = await readFile(
    new URL("../../styles/features/chat/navigation.css", import.meta.url),
    "utf8",
  );
  const trackRule = css.match(/\.chat-msg-nav-track\s*\{(?<body>[\s\S]*?)\}/);

  assert.ok(trackRule?.groups?.body, "missing chat message navigation track rule");
  assert.match(
    trackRule.groups.body,
    /padding:\s*10px 28px 10px 0;/,
    "the track must expand its hit area toward the conversation",
  );
});

test("chat turn navigation is anchored to the left edge", async () => {
  const css = await readFile(
    new URL("../../styles/features/chat/navigation.css", import.meta.url),
    "utf8",
  );
  const navRule = css.match(/\.chat-msg-nav\s*\{(?<body>[\s\S]*?)\}/);

  assert.ok(navRule?.groups?.body, "missing chat message navigation rule");
  assert.match(navRule.groups.body, /left:\s*14px;/);
  assert.doesNotMatch(navRule.groups.body, /right:/);
});

test("chat anchor labels use a translucent blurred surface", async () => {
  const css = await readFile(
    new URL("../../styles/features/chat/navigation.css", import.meta.url),
    "utf8",
  );
  const labelRule = css.match(/\.chat-msg-nav-label\s*\{(?<body>[\s\S]*?)\}/);

  assert.ok(labelRule?.groups?.body, "missing chat message navigation label rule");
  assert.match(labelRule.groups.body, /background:[\s\S]*var\(--nav-label-bg\)/);
  assert.doesNotMatch(
    labelRule.groups.body,
    /linear-gradient\(/,
    "the glass label must not add a gradient overlay",
  );
  assert.match(labelRule.groups.body, /backdrop-filter:\s*blur\(/);
  assert.match(labelRule.groups.body, /-webkit-backdrop-filter:\s*blur\(/);
});
