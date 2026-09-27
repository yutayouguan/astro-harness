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
  assert.doesNotMatch(panel, /onRegenerateMessage|regenerateMessage/);
  assert.match(panel, /onEditUserMessage=\{chat\.editUserMessage\}/);
  assert.doesNotMatch(panel, /onDeleteMessage|chat\.deleteMessage/);

  assert.doesNotMatch(panel, /side-chat-msg/);
  assert.doesNotMatch(panel, /side-chat-composer/);
  assert.doesNotMatch(panel, /listen<StreamPayload>/);
});

test("side chat keeps backend context but never overwrites the primary client snapshot", async () => {
  const [sideChatHook, panel, sessionHook] = await Promise.all([
    source("hooks/chat/useSideChatSession.ts"),
    source("components/chat/SideChatPanel.tsx"),
    source("hooks/chat/useChatSession.ts"),
  ]);

  assert.match(
    sideChatHook,
    /invoke<string>\("fork_chat_session", \{[\s\S]*?ephemeral: true,[\s\S]*?excludeTurns: true,/,
  );
  assert.match(
    sideChatHook,
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

test("discarding the side chat always asks the user first", async () => {
  const [app, zh, en] = await Promise.all([
    source("App.tsx"),
    source("i18n/catalogs/zh.ts"),
    source("i18n/catalogs/en.ts"),
  ]);

  // 唯一的 discard 入口带确认，取消时只让位、不结束会话
  assert.match(app, /const discardSideChat = useCallback\(async \(\) => \{/);
  assert.match(
    app,
    /const confirmed = await confirm\(\{[\s\S]*?"chat\.side\.closeConfirmTitle"[\s\S]*?variant: "danger",[\s\S]*?\}\);\s*\n\s*if \(!confirmed\) return;\s*\n\s*await closeSideChat\(\);/,
  );

  const discards = app.match(/closeSideChat\(\)/g) ?? [];
  assert.equal(
    discards.length,
    1,
    "侧边会话只允许在确认后的 helper 里被丢弃",
  );

  // 所有入口（右侧坞切换与显式关闭）都走确认
  assert.match(app, /void discardSideChat\(\);/);
  assert.match(
    app,
    /void \(sideSessionId \? discardSideChat\(\) : startSideChat\(\)\)/,
  );
  assert.match(app, /onClose=\{\(\) => void discardSideChat\(\)\}/);

  for (const key of [
    "chat.side.closeConfirmTitle",
    "chat.side.closeConfirm",
    "chat.side.closeConfirmAction",
    "chat.side.closeKeep",
  ]) {
    assert.ok(zh.includes(`"${key}":`), `zh missing ${key}`);
    assert.ok(en.includes(`"${key}":`), `en missing ${key}`);
  }
});
