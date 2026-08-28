import assert from "node:assert/strict";
import test from "node:test";
import type { ChatMessage } from "../../types.ts";
import {
  findLastUserMessageId,
  findLastUserMessageIndex,
} from "./messageEditing.ts";

test("finds only the latest real user question", () => {
  const messages: ChatMessage[] = [
    { id: "welcome", role: "assistant", content: "hello" },
    { id: "u-1", role: "user", content: "first" },
    { id: "a-1", role: "assistant", content: "answer" },
    { id: "u-2", role: "user", content: "latest" },
    { id: "a-2", role: "assistant", content: "answer" },
  ];

  assert.equal(findLastUserMessageIndex(messages), 3);
  assert.equal(findLastUserMessageId(messages), "u-2");
});

test("returns no editable question for an empty conversation", () => {
  assert.equal(findLastUserMessageIndex([]), -1);
  assert.equal(findLastUserMessageId([]), null);
});
