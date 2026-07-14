import { test } from "node:test";
import assert from "node:assert/strict";
import {
  applyReasoningDelta,
  applyActivityUpsert,
  applySurfaceUpsert,
  sealOpenReasoning,
  sumReasoningDurations,
} from "./chatTimeline.ts";
import type { ChatMessage } from "../types.ts";

function emptyAssistant(id = "a1"): ChatMessage {
  return { id, role: "assistant", content: "" };
}

test("reasoning then tool then reasoning creates three segments", () => {
  let m = emptyAssistant();
  m = applyReasoningDelta(m, "think1", 100);
  m = applyActivityUpsert(m, {
    id: "c1",
    kind: "tool",
    title: "present_ui",
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
    title: "present_ui",
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
    title: "terminal",
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
    title: "terminal",
    status: "running",
    at: 10_000,
  });
  m = applyActivityUpsert(m, {
    id: "c1",
    kind: "tool",
    title: "terminal",
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
