import assert from "node:assert/strict";
import test from "node:test";
import { groupAssistantAnswer } from "./groupAssistantAnswer.ts";

test("projects interleaved assistant segments into categorized content", () => {
  const grouped = groupAssistantAnswer({
    content: "第一段第二段",
    reasoning: "思考甲思考乙",
    segments: [
      { type: "reasoning", id: "r1", text: "思考甲", at: 1, durationSec: 1.2 },
      { type: "text", id: "t1", text: "第一段", at: 2 },
      { type: "activity", id: "a1", at: 3 },
      { type: "reasoning", id: "r2", text: "思考乙", at: 4, durationSec: 0.8 },
      { type: "text", id: "t2", text: "第二段", at: 5 },
    ],
  });

  assert.deepEqual(grouped, {
    reasoning: "思考甲思考乙",
    reasoningDurationSec: 2,
    text: "第一段第二段",
  });
});

test("keeps canonical aggregates when they differ from recovery segments", () => {
  const grouped = groupAssistantAnswer({
    content: "完整回答",
    reasoning: "完整思考",
    reasoningDurationSec: 3.5,
    segments: [
      { type: "reasoning", id: "r1", text: "旧思考", at: 1, durationSec: 1 },
      { type: "text", id: "t1", text: "旧回答", at: 2 },
    ],
  });

  assert.deepEqual(grouped, {
    reasoning: "完整思考",
    reasoningDurationSec: 3.5,
    text: "完整回答",
  });
});
