import { test } from "node:test";
import assert from "node:assert/strict";
import {
  applyReasoningDelta,
  applyActivityUpsert,
  applySurfaceUpsert,
  coalesceReasoningSegments,
  reconcileReasoning,
  sealOpenReasoning,
  sumReasoningDurations,
} from "./chatTimeline.ts";
import type { ChatMessage, ChatTimelineSegment } from "../../types.ts";

function emptyAssistant(id = "a1"): ChatMessage {
  return { id, role: "assistant", content: "" };
}

test("reasoning then tool then reasoning creates three segments", () => {
  let m = emptyAssistant();
  m = applyReasoningDelta(m, "think1", 100);
  m = applyActivityUpsert(m, {
    id: "c1",
    kind: "tool",
    title: "present",
    status: "running",
    at: 200,
  });
  m = applyReasoningDelta(m, "think2", 300);
  assert.equal(m.segments?.length, 3);
  assert.equal(m.segments?.[0]?.type, "reasoning");
  assert.equal(m.segments?.[1]?.type, "activity");
  assert.equal(m.segments?.[2]?.type, "reasoning");
  assert.equal((m.segments?.[0] as { text: string }).text, "think1");
  assert.equal((m.segments?.[2] as { text: string }).text, "think2");
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

test("coalesceReasoningSegments merges all reasoning into one at first position", () => {
  const segments: ChatTimelineSegment[] = [
    { type: "reasoning", id: "r1", text: "think1", at: 100, durationSec: 1 },
    { type: "activity", id: "t1", at: 200 },
    { type: "reasoning", id: "r2", text: "think2", at: 300, durationSec: 2 },
    { type: "activity", id: "t2", at: 400 },
    { type: "reasoning", id: "r3", text: "think3", at: 500, durationSec: 0.5 },
  ];
  const out = coalesceReasoningSegments(segments);
  assert.equal(out?.length, 3);
  assert.equal(out?.[0]?.type, "reasoning");
  if (out?.[0]?.type === "reasoning") {
    assert.equal(out[0].text, "think1think2think3");
    assert.equal(out[0].id, "r1");
    assert.equal(out[0].at, 100);
    assert.equal(out[0].durationSec, 3.5);
  }
  assert.equal(out?.[1]?.type, "activity");
  assert.equal(out?.[1]?.type === "activity" ? out[1].id : null, "t1");
  assert.equal(out?.[2]?.type, "activity");
  assert.equal(out?.[2]?.type === "activity" ? out[2].id : null, "t2");
});

test("coalesceReasoningSegments leaves single reasoning unchanged", () => {
  const segments: ChatTimelineSegment[] = [
    { type: "reasoning", id: "r1", text: "only", at: 1, durationSec: 2 },
    { type: "activity", id: "t1", at: 2 },
  ];
  const out = coalesceReasoningSegments(segments);
  assert.deepEqual(out, segments);
});

test("coalesceReasoningSegments keeps last open (no duration) while streaming", () => {
  const segments: ChatTimelineSegment[] = [
    { type: "reasoning", id: "r1", text: "a", at: 1000, durationSec: 1 },
    { type: "activity", id: "t1", at: 2000 },
    { type: "reasoning", id: "r2", text: "b", at: 3000 },
  ];
  const out = coalesceReasoningSegments(segments);
  assert.equal(out?.[0]?.type, "reasoning");
  if (out?.[0]?.type === "reasoning") {
    assert.equal(out[0].text, "ab");
    assert.equal(out[0].durationSec, undefined);
    assert.equal(out[0].at, 1000);
  }
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
  assert.equal(reconciled.segments?.filter((segment) => segment.type === "activity").length, 1);
});
