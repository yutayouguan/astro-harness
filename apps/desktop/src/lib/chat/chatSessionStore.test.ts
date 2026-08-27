/**
 * 编辑截断后本地持久化：避免空列表触发 restore 时把旧历史拉回。
 */
import assert from "node:assert/strict";
import { afterEach, before, test } from "node:test";
import type { ChatMessage } from "../../types.ts";
import {
  isChatCleared,
  loadChatSession,
  loadContextUsageForSession,
  persistAfterEditTruncate,
  saveChatSession,
} from "./chatSessionStore.ts";

const STORAGE_KEY = "astro.chat.session";
const CLEARED_KEY = "astro.chat.cleared";

const store = new Map<string, string>();

before(() => {
  const g = globalThis as { localStorage?: Storage };
  g.localStorage = {
    getItem: (k) => store.get(k) ?? null,
    setItem: (k, v) => {
      store.set(k, v);
    },
    removeItem: (k) => {
      store.delete(k);
    },
    clear: () => store.clear(),
    key: () => null,
    length: 0,
  };
});

afterEach(() => {
  store.clear();
});

function msg(id: string, role: "user" | "assistant", content: string): ChatMessage {
  return { id, role, content, createdAt: 1 };
}

test("persistAfterEditTruncate(keep=0) marks cleared so restore cannot reload old session", () => {
  saveChatSession("s1", [
    msg("u1", "user", "给我生成一个中国古代美女，在御剑飞行"),
    msg("a1", "assistant", "来嘞"),
  ]);
  assert.ok(loadChatSession());

  persistAfterEditTruncate("s1", []);

  assert.equal(isChatCleared(), true);
  assert.equal(loadChatSession(), null);
  assert.equal(store.has(STORAGE_KEY), false);
  assert.equal(store.get(CLEARED_KEY), "1");
});

test("persistAfterEditTruncate(keep>0) writes kept prefix and clears cleared flag", () => {
  store.set(CLEARED_KEY, "1");
  const kept = [msg("u1", "user", "第一轮"), msg("a1", "assistant", "答")];

  persistAfterEditTruncate("s2", kept);

  assert.equal(isChatCleared(), false);
  const stored = loadChatSession();
  assert.ok(stored);
  assert.equal(stored!.sessionId, "s2");
  assert.equal(stored!.messages.length, 2);
  assert.equal(stored!.messages[0]!.content, "第一轮");
});

test("saveChatSession records contextUsage and preserves it when omitted", () => {
  const usage = {
    contextWindow: 1_048_576,
    totalTokens: 12_345,
    segments: [{ id: "conversation", tokens: 12_345 }],
    updatedAt: 42,
  };
  saveChatSession("s3", [msg("u1", "user", "hi"), msg("a1", "assistant", "yo")], [], usage);
  let stored = loadChatSession();
  assert.ok(stored?.contextUsage);
  assert.equal(stored!.contextUsage!.contextWindow, 1_048_576);
  assert.equal(stored!.contextUsage!.totalTokens, 12_345);

  saveChatSession("s3", [msg("u1", "user", "hi"), msg("a1", "assistant", "yo2"), msg("u2", "user", "again")]);
  stored = loadChatSession();
  assert.equal(stored!.contextUsage!.totalTokens, 12_345);

  saveChatSession("s3", [msg("u1", "user", "hi"), msg("a1", "assistant", "yo2")], [], null);
  stored = loadChatSession();
  assert.equal(stored!.contextUsage, undefined);
});

test("loadContextUsageForSession restores per-session cache", () => {
  const usage = {
    contextWindow: 128_000,
    totalTokens: 99,
    segments: [{ id: "tools", tokens: 99, items: [{ id: "exec_command", label: "exec_command", tokens: 99 }] }],
    updatedAt: 7,
  };
  saveChatSession("sess-a", [msg("u1", "user", "a"), msg("a1", "assistant", "b")], [], usage);
  assert.equal(loadContextUsageForSession("sess-a")?.totalTokens, 99);
  assert.equal(loadContextUsageForSession("sess-b"), null);

  saveChatSession("sess-a", [msg("u1", "user", "a"), msg("a1", "assistant", "b")], [], null);
  assert.equal(loadContextUsageForSession("sess-a"), null);
});
