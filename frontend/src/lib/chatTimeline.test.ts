import { test } from "node:test";
import assert from "node:assert/strict";
import {
  applyReasoningDelta,
  applyActivityUpsert,
  applySurfaceUpsert,
  sealOpenReasoning,
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

test("sealOpenReasoning writes duration even when last segment is activity", () => {
  let m = emptyAssistant();
  m = applyReasoningDelta(m, "think", 100);
  m = applyActivityUpsert(m, {
    id: "c1",
    kind: "tool",
    title: "tool",
    status: "done",
    at: 200,
  });
  m = sealOpenReasoning(m, 3.3);
  assert.equal(m.reasoningDurationSec, 3.3);
  const r = m.segments?.find((s) => s.type === "reasoning") as
    | { durationSec?: number }
    | undefined;
  assert.equal(r?.durationSec, 3.3);
});
