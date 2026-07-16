/**
 * 编辑截断后本地持久化：避免空列表触发 restore 时把旧历史拉回。
 */
import assert from "node:assert/strict";
import { afterEach, before, test } from "node:test";
import type { ChatMessage } from "../types.ts";
import {
  isChatCleared,
  loadChatSession,
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
