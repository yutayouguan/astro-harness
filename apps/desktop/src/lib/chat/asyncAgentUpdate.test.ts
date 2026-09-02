import { test } from "node:test";
import assert from "node:assert/strict";
import {
  pendingAsyncQuestionsAt,
  upsertAsyncAgentUpdate,
} from "./asyncAgentUpdate.ts";
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
    undefined,
    42,
  );
  const replayed = upsertAsyncAgentUpdate(
    inserted,
    "assistant-final",
    "call-1:async-message",
    "Still working",
    undefined,
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

test("async message preserves structured questions across replay", () => {
  const questions = [{ title: "Choose a target", options: ["A", "B"] }];
  const inserted = upsertAsyncAgentUpdate(
    [{ id: "assistant-final", role: "assistant", content: "" }],
    "assistant-final",
    "call-2:async-message",
    "Choose a target\n- A\n- B",
    questions,
    42,
  );

  assert.deepEqual(inserted[0]?.asyncQuestions, questions);
});

test("only the latest unanswered async question group stays interactive", () => {
  const first: ConversationEntry = {
    id: "first",
    role: "assistant",
    content: "First?",
    asyncQuestions: [{ title: "First?" }],
  };
  const latest: ConversationEntry = {
    id: "latest",
    role: "assistant",
    content: "Latest?",
    asyncQuestions: [{ title: "Latest?" }],
  };
  assert.equal(pendingAsyncQuestionsAt([first, latest], 0), undefined);
  assert.deepEqual(pendingAsyncQuestionsAt([first, latest], 1), latest.asyncQuestions);
  assert.equal(
    pendingAsyncQuestionsAt(
      [latest, { id: "reply", role: "user", content: "answer" }],
      0,
    ),
    undefined,
  );
});
