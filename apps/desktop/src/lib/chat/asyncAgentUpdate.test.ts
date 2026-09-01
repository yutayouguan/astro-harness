import { test } from "node:test";
import assert from "node:assert/strict";
import { upsertAsyncAgentUpdate } from "./asyncAgentUpdate.ts";
import type { ConversationEntry } from "../../types.ts";

test("async message is inserted before the active response and deduplicated", () => {
  const messages: ConversationEntry[] = [
    { id: "user-1", role: "user", content: "go" },
    { id: "assistant-final", role: "assistant", content: "" },
  ];

  const inserted = upsertAsyncAgentUpdate(
    messages,
    "assistant-final",
    "call-1:async-message",
    "Still working",
    42,
  );
  const replayed = upsertAsyncAgentUpdate(
    inserted,
    "assistant-final",
    "call-1:async-message",
    "Still working",
    99,
  );

  assert.deepEqual(replayed, [
    messages[0],
    {
      id: "call-1:async-message",
      role: "assistant",
      content: "Still working",
      delivery: "async",
      createdAt: 42,
    },
    messages[1],
  ]);
});
