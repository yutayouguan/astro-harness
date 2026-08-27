import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const headerStyles = await readFile(
  new URL("../../styles/features/shell/header.css", import.meta.url),
  "utf8",
);
const chatStyles = await readFile(
  new URL("../../styles/features/chat/markdown.css", import.meta.url),
  "utf8",
);
const coreStyles = await readFile(
  new URL("../../styles/features/chat/core.css", import.meta.url),
  "utf8",
);
const chatView = await readFile(
  new URL("../../components/chat/ChatView.tsx", import.meta.url),
  "utf8",
);

function rule(css, selector) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return css.match(
    new RegExp(`(?:^|\\n)${escaped}\\s*\\{(?<body>[\\s\\S]*?)\\}`),
  )?.groups?.body;
}

test("chat header controls float without taking horizontal layout space", () => {
  const actions = rule(headerStyles, ".content-header--chat .header-actions");

  assert.ok(actions, "missing chat header action rule");
  assert.match(actions, /position:\s*absolute;/);
  assert.match(actions, /right:\s*28px;/);
});

test("composer floats above a full-height conversation viewport", () => {
  const pane = rule(headerStyles, ".chat-pane");
  const composer = rule(chatStyles, ".composer-shell");
  const messages = rule(coreStyles, ".message-list");

  assert.ok(pane, "missing chat pane rule");
  assert.match(pane, /position:\s*relative;/);
  assert.ok(composer, "missing composer shell rule");
  assert.match(composer, /position:\s*absolute;/);
  assert.match(composer, /bottom:\s*14px;/);
  assert.ok(messages, "missing message list rule");
  assert.match(messages, /--composer-overlay-height/);
  assert.match(messages, /scroll-padding-block-end:/);
});

test("composer overlay clearance follows the live composer height", () => {
  assert.match(chatView, /new ResizeObserver\(syncComposerOverlayHeight\)/);
  assert.match(chatView, /--composer-overlay-height/);
  assert.match(chatView, /ref=\{composerShellRef\}/);
});
