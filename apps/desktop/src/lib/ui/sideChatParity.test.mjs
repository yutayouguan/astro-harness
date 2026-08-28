import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const root = new URL("../../", import.meta.url);

async function source(path) {
  return readFile(new URL(path, root), "utf8");
}

test("side chat mounts the complete main chat surface", async () => {
  const panel = await source("components/chat/SideChatPanel.tsx");

  assert.match(panel, /import ChatView from "\.\/ChatView"/);
  assert.match(panel, /const chat = useChatSession\(\{/);
  assert.match(panel, /<ChatView/);
  assert.match(panel, /onAttachmentsChange=\{chat\.setAttachments\}/);
  assert.match(panel, /pendingInterrupts=\{chat\.sessionPendingInterrupts\}/);
  assert.match(panel, /onUiAction=\{chat\.onUiAction\}/);
  assert.match(panel, /onOpenMcpSettings=\{onOpenMcpSettings\}/);
  assert.match(panel, /onRegenerateMessage=\{chat\.regenerateMessage\}/);
  assert.match(panel, /onEditUserMessage=\{chat\.editUserMessage\}/);
  assert.doesNotMatch(panel, /onDeleteMessage|chat\.deleteMessage/);

  assert.doesNotMatch(panel, /side-chat-msg/);
  assert.doesNotMatch(panel, /side-chat-composer/);
  assert.doesNotMatch(panel, /listen<StreamPayload>/);
});

test("side chat keeps backend context but never overwrites the primary client snapshot", async () => {
  const [app, panel, sessionHook] = await Promise.all([
    source("App.tsx"),
    source("components/chat/SideChatPanel.tsx"),
    source("hooks/chat/useChatSession.ts"),
  ]);

  assert.match(
    app,
    /invoke<string>\("fork_chat_session", \{[\s\S]*?ephemeral: true,[\s\S]*?excludeTurns: true,/,
  );
  assert.match(
    app,
    /invoke\("discard_side_session", \{ sessionId: sideId \}\)/,
  );
  assert.match(panel, /persistClientState: false/);
  assert.match(panel, /initialSessionId: sessionId/);
  assert.match(panel, /initialEphemeral: true/);
  assert.match(
    sessionHook,
    /if \(!persistClientState \|\| restoringRef\.current \|\| streaming\) return;/,
  );
  assert.match(sessionHook, /persistContextUsage: persistClientState/);
});
