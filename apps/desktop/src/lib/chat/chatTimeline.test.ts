import { test } from "node:test";
import assert from "node:assert/strict";
import {
  applyReasoningDelta,
  applyActivityUpsert,
  applySurfaceUpsert,
  projectCanonicalTimelineSegments,
  reconcileReasoning,
  reconcileText,
  sealOpenReasoning,
  sumReasoningDurations,
  applyTextDelta,
} from "./chatTimeline.ts";
import type { ConversationEntry } from "../../types.ts";

function emptyAssistant(id = "a1"): ConversationEntry {
  return { id, role: "assistant", content: "" };
}

test("reasoning, tools, text, and later reasoning preserve event order", () => {
  let m = emptyAssistant();
  m = applyReasoningDelta(m, "think1", 100);
  m = applyActivityUpsert(m, {
    id: "c1",
    kind: "tool",
    title: "present",
    status: "running",
    at: 200,
  });
  m = applyTextDelta(m, "partial answer", 300);
  m = applyReasoningDelta(m, "think2", 400);
  assert.equal(m.segments?.length, 4);
  assert.equal(m.segments?.[0]?.type, "reasoning");
  assert.equal(m.segments?.[1]?.type, "activity");
  assert.equal(m.segments?.[2]?.type, "text");
  assert.equal(m.segments?.[3]?.type, "reasoning");
  assert.equal((m.segments?.[0] as { text: string }).text, "think1");
  assert.equal((m.segments?.[2] as { text: string }).text, "partial answer");
  assert.equal((m.segments?.[3] as { text: string }).text, "think2");
  assert.equal(m.content, "partial answer");
  assert.equal(m.reasoning, "think1think2");
  assert.equal(m.activities?.length, 1);
});

test("activity upsert same id does not duplicate segment", () => {
  let m = emptyAssistant();
  m = applyActivityUpsert(m, {
    id: "c1",
    kind: "tool",
    title: "x",
    status: "running",
    at: 1,
  });
  m = applyActivityUpsert(m, {
    id: "c1",
    kind: "tool",
    title: "x",
    output: "ok",
    status: "done",
    at: 2,
  });
  assert.equal(m.segments?.filter((s) => s.type === "activity").length, 1);
  assert.equal(m.activities?.[0]?.output, "ok");
});

test("surface after activity appends surface segment", () => {
  let m = emptyAssistant();
  m = applyActivityUpsert(m, {
    id: "c1",
    kind: "tool",
    title: "present",
    at: 1,
  });
  m = applySurfaceUpsert(
    m,
    {
      messageId: "a2ui-surface-c1",
      activityType: "a2ui-surface",
      operations: [{ version: "v0.9" }],
      status: "active",
    },
    2,
  );
  assert.equal(m.segments?.at(-1)?.type, "surface");
  assert.equal(m.uiSurfaces?.length, 1);
});

test("tool interrupt seals first reasoning with segment wall-clock", () => {
  let m = emptyAssistant();
  m = applyReasoningDelta(m, "think1", 1000);
  m = applyActivityUpsert(m, {
    id: "c1",
    kind: "tool",
    title: "exec_command",
    status: "running",
    at: 3500,
  });
  const r0 = m.segments?.[0] as { type: string; durationSec?: number };
  assert.equal(r0.type, "reasoning");
  assert.equal(r0.durationSec, 2.5);
  m = applyReasoningDelta(m, "think2", 5000);
  m = sealOpenReasoning(m, 6200);
  const r1 = m.segments?.[2] as { type: string; durationSec?: number };
  assert.equal(r1?.type, "reasoning");
  assert.equal(r1?.durationSec, 1.2);
  assert.equal(sumReasoningDurations(m.segments), 3.7);
  assert.equal(m.reasoningDurationSec, 3.7);
});

test("tool done preserves at and writes durationSec", () => {
  let m = emptyAssistant();
  m = applyActivityUpsert(m, {
    id: "c1",
    kind: "tool",
    title: "exec_command",
    status: "running",
    at: 10_000,
  });
  m = applyActivityUpsert(m, {
    id: "c1",
    kind: "tool",
    title: "exec_command",
    output: "ok",
    status: "done",
    at: 99_000,
    durationSec: 1.5,
  });
  assert.equal(m.activities?.[0]?.at, 10_000);
  assert.equal(m.activities?.[0]?.durationSec, 1.5);
  assert.equal(m.activities?.[0]?.status, "done");
});

test("sealOpenReasoning does not paint total onto sealed earlier segments", () => {
  let m = emptyAssistant();
  m = applyReasoningDelta(m, "a", 1000);
  m = applyActivityUpsert(m, {
    id: "t1",
    kind: "tool",
    title: "x",
    status: "running",
    at: 2000,
  });
  m = applyReasoningDelta(m, "b", 3000);
  m = sealOpenReasoning(m, 4000);
  const durs = (m.segments ?? [])
    .filter((s) => s.type === "reasoning")
    .map((s) => (s.type === "reasoning" ? s.durationSec : undefined));
  assert.deepEqual(durs, [1, 1]);
});

test("text deltas coalesce only while adjacent", () => {
  let m = emptyAssistant();
  m = applyTextDelta(m, "a", 100);
  m = applyTextDelta(m, "b", 110);
  m = applyReasoningDelta(m, "think", 200);
  m = applyTextDelta(m, "c", 300);

  assert.deepEqual(
    m.segments?.map((segment) =>
      segment.type === "activity" || segment.type === "surface"
        ? segment.type
        : `${segment.type}:${segment.text}`,
    ),
    ["text:ab", "reasoning:think", "text:c"],
  );
  assert.equal(m.content, "abc");
});

test("text reconciliation preserves prior interleaving when only the tail changes", () => {
  let m = emptyAssistant();
  m = applyTextDelta(m, "before", 100);
  m = applyActivityUpsert(m, {
    id: "t1",
    kind: "tool",
    title: "x",
    status: "done",
    at: 200,
  });
  m = applyTextDelta(m, "draft", 300);

  const reconciled = reconcileText(m, "beforefinal", 400);
  assert.deepEqual(
    reconciled.segments?.map((segment) => segment.type),
    ["text", "activity", "text"],
  );
  assert.equal(reconciled.segments?.[2]?.type, "text");
  if (reconciled.segments?.[2]?.type === "text") {
    assert.equal(reconciled.segments[2].text, "final");
  }
});

test("timeline projection restores a missing canonical answer tail without reordering tools", () => {
  const original: ConversationEntry = {
    id: "a1",
    role: "assistant",
    content: "先写脚本。\n\n脚本已写好。\n\n最终答案。",
    reasoning: "分析验证",
    activities: [
      { id: "t1", kind: "tool", title: "apply_patch", at: 2 },
      { id: "t2", kind: "tool", title: "terminal", at: 5 },
    ],
    segments: [
      { type: "reasoning", id: "r1", text: "分析", at: 1 },
      { type: "activity", id: "t1", at: 2 },
      { type: "text", id: "txt1", text: "先写脚本。", at: 3 },
      { type: "reasoning", id: "r2", text: "验证", at: 4 },
      { type: "activity", id: "t2", at: 5 },
      { type: "text", id: "txt2", text: "脚本已写好。", at: 6 },
    ],
  };

  const projected = projectCanonicalTimelineSegments(original);

  assert.deepEqual(
    projected.map((segment) => segment.type),
    ["reasoning", "activity", "text", "reasoning", "activity", "text"],
  );
  assert.equal(
    projected
      .filter((segment) => segment.type === "text")
      .map((segment) => segment.text)
      .join(" ")
      .replace(/\s+/g, " ")
      .trim(),
    original.content.replace(/\s+/g, " ").trim(),
  );
  assert.equal(original.segments?.at(-1)?.type, "text");
  assert.equal(
    original.segments?.at(-1)?.type === "text"
      ? original.segments.at(-1)?.text
      : "",
    "脚本已写好。",
  );
});

test("timeline projection appends a missing final answer after a trailing tool", () => {
  const projected = projectCanonicalTimelineSegments({
    id: "a2",
    role: "assistant",
    content: "准备。最终答案。",
    activities: [{ id: "t1", kind: "tool", title: "terminal", at: 2 }],
    segments: [
      { type: "text", id: "txt1", text: "准备。", at: 1 },
      { type: "activity", id: "t1", at: 2 },
    ],
  });

  assert.deepEqual(
    projected.map((segment) => segment.type),
    ["text", "activity", "text"],
  );
  assert.equal(
    projected[2]?.type === "text" ? projected[2].text : "",
    "最终答案。",
  );
});

test("timeline projection never collapses interleaved events on divergent text", () => {
  const segments: ConversationEntry["segments"] = [
    { type: "reasoning", id: "r1", text: "第一次思考", at: 1 },
    { type: "text", id: "txt1", text: "中间说明", at: 2 },
    { type: "activity", id: "t1", at: 3 },
    { type: "reasoning", id: "r2", text: "第二次思考", at: 4 },
  ];
  const projected = projectCanonicalTimelineSegments({
    id: "a3",
    role: "assistant",
    content: "完全不同的最终正文",
    reasoning: "归类后的思考文本",
    segments,
  });

  assert.deepEqual(projected, segments);
});

test("canonical reasoning reconciliation replaces divergent text and keeps activities", () => {
  let m = emptyAssistant();
  m = applyReasoningDelta(m, "hel", 100);
  m = applyActivityUpsert(m, {
    id: "t1",
    kind: "tool",
    title: "x",
    status: "running",
    at: 200,
  });
  m = applyReasoningDelta(m, " world", 300);

  const reconciled = reconcileReasoning(m, "hello world", 400);
  assert.equal(reconciled.reasoning, "hello world");
  assert.equal(
    reconciled.segments
      ?.filter((segment) => segment.type === "reasoning")
      .map((segment) => (segment.type === "reasoning" ? segment.text : ""))
      .join(""),
    "hello world",
  );
  assert.deepEqual(
    reconciled.segments?.map((segment) => segment.type),
    ["reasoning", "activity", "reasoning"],
  );
  assert.equal(
    reconciled.segments?.filter((segment) => segment.type === "activity")
      .length,
    1,
  );
});
