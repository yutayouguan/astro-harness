import assert from "node:assert/strict";
import test from "node:test";
import type { ConversationEntry } from "../../types.ts";
import { findLastUserEntryId, findLastUserEntryIndex } from "./turnEditing.ts";

test("finds only the latest real user question", () => {
  const messages: ConversationEntry[] = [
    { id: "welcome", role: "assistant", content: "hello" },
    { id: "u-1", role: "user", content: "first" },
    { id: "a-1", role: "assistant", content: "answer" },
    { id: "u-2", role: "user", content: "latest" },
    { id: "a-2", role: "assistant", content: "answer" },
  ];

  assert.equal(findLastUserEntryIndex(messages), 3);
  assert.equal(findLastUserEntryId(messages), "u-2");
});

test("returns no editable question for an empty conversation", () => {
  assert.equal(findLastUserEntryIndex([]), -1);
  assert.equal(findLastUserEntryId([]), null);
});
