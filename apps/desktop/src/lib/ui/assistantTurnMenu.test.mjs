import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const root = new URL("../../", import.meta.url);

async function source(path) {
  return readFile(new URL(path, root), "utf8");
}

test("assistant answers expose one accessible context menu from pointer and toolbar", async () => {
  const [chatView, menu] = await Promise.all([
    source("components/chat/ChatView.tsx"),
    source("components/chat/AssistantTurnContextMenu.tsx"),
  ]);

  assert.match(chatView, /onContextMenu=\{\(event\) =>/);
  assert.match(chatView, /respectTextSelection/);
  assert.match(chatView, /aria-haspopup="menu"/);
  assert.match(menu, /role="menu"/);
  assert.match(menu, /role="menuitemradio"/);
  assert.match(menu, /\["ArrowDown", "ArrowUp", "Home", "End"\]/);
  assert.match(menu, /event\.key === "Escape"/);
  assert.match(menu, /onClose\(true\)/);
  assert.match(chatView, /assistantMenuReturnFocusRef/);
  assert.match(chatView, /returnFocus\.focus\(\)/);
});

test("per-answer layout overrides remain separate from the global default", async () => {
  const chatView = await source("components/chat/ChatView.tsx");

  assert.match(
    chatView,
    /messageLayoutOverrides\[m\.id\] \?\? displayPrefs\.answerLayout/,
  );
  assert.match(chatView, /onDefaultAnswerLayoutChange\(layout\)/);
  assert.match(chatView, /action === "layout-default"/);
  assert.match(
    chatView,
    /action === "layout-timeline" \|\| action === "layout-grouped"/,
  );
  assert.match(chatView, /setMessageLayoutOverrides\(\{\}\)/);
  assert.match(chatView, /setMessageProcessExpanded\(\{\}\)/);
});

test("copy actions are disabled when an answer has no text", async () => {
  const [chatView, menu] = await Promise.all([
    source("components/chat/ChatView.tsx"),
    source("components/chat/AssistantTurnContextMenu.tsx"),
  ]);

  assert.match(
    chatView,
    /hasAnswer=\{Boolean\(contextMenuMessage\.content\.trim\(\)\)\}/,
  );
  assert.match(menu, /action: "copy-answer"[\s\S]*?disabled: !hasAnswer/);
  assert.match(menu, /action: "copy-markdown"[\s\S]*?disabled: !hasAnswer/);
});

test("side chat can promote a per-answer layout to the global default", async () => {
  const [app, sideChat] = await Promise.all([
    source("App.tsx"),
    source("components/chat/SideChatPanel.tsx"),
  ]);

  assert.match(
    app,
    /<SideChatPanel[\s\S]*?onDefaultAnswerLayoutChange=\{setAnswerLayout\}/,
  );
  assert.match(
    sideChat,
    /<ChatView[\s\S]*?onDefaultAnswerLayoutChange=\{onDefaultAnswerLayoutChange\}/,
  );
});

test("full-process controls reach reasoning and tool groups", async () => {
  const [chatView, reasoning, activities] = await Promise.all([
    source("components/chat/ChatView.tsx"),
    source("components/chat/MsgReasoning.tsx"),
    source("components/chat/MsgActivityGroup.tsx"),
  ]);

  assert.match(chatView, /forcedOpen=\{forcedProcessOpen\}/);
  assert.match(
    chatView,
    /current\[message\.id\] \?\? displayPrefs\.processDefaultOpen/,
  );
  assert.match(
    chatView,
    /defaultOpen=\{[\s\S]*?displayPrefs\.processDefaultOpen[\s\S]*?\}/,
  );
  assert.match(reasoning, /forcedOpen\?: boolean/);
  assert.match(reasoning, /defaultOpen\?: boolean/);
  assert.match(activities, /forcedOpen\?: boolean/);
  assert.match(activities, /defaultOpen\?: boolean/);
  assert.match(activities, /else \{[\s\S]*?setOpen\(defaultOpen\);[\s\S]*?\}/);
});

test("menu glass stays responsive and respects motion and transparency preferences", async () => {
  const [css, tokens] = await Promise.all([
    source("styles/features/chat/assistant-turn-context-menu.css"),
    source("styles/tokens/component/menu.css"),
  ]);

  assert.match(css, /width: min\(238px, calc\(100vw - 16px\)\)/);
  assert.match(css, /background:\s*var\(--menu-overlay-bg\)/);
  assert.match(css, /backdrop-filter:\s*var\(--menu-overlay-blur\)/);
  assert.match(tokens, /--menu-overlay-bg:/);
  assert.match(tokens, /var\(--bg1\) 92%/);
  assert.match(tokens, /--menu-overlay-blur:\s*blur\(calc\(28px/);
  assert.match(css, /prefers-reduced-motion: reduce/);
  assert.match(css, /prefers-reduced-transparency: reduce/);
});
