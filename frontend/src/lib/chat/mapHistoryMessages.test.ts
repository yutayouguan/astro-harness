import assert from "node:assert/strict";
import test from "node:test";
import {
  coalesceConsecutiveAssistants,
  enrichTimelineDurations,
  mapHistoryMessages,
} from "./mapHistoryMessages.ts";

test("mapHistoryMessages restores activity media from history DTO", () => {
  const msgs = mapHistoryMessages([
    {
      id: "db-1",
      role: "assistant",
      content: "done",
      activities: [
        {
          id: "c1",
          kind: "tool",
          title: "image_gen",
          output: "图片已生成：generated/images/a.jpg",
          status: "done",
          media: [{ kind: "image", path: "generated/images/a.jpg" }],
        },
      ],
    },
  ]);
  assert.equal(msgs.length, 1);
  assert.deepEqual(msgs[0]!.activities?.[0]?.media, [
    { kind: "image", path: "generated/images/a.jpg" },
  ]);
});

test("mapHistoryMessages drops invalid media entries", () => {
  const msgs = mapHistoryMessages([
    {
      id: "db-1",
      role: "assistant",
      content: "",
      activities: [
        {
          id: "c1",
          kind: "tool",
          title: "image_gen",
          status: "done",
          media: [
            { kind: "image", path: "ok.jpg" },
            { kind: "nope", path: "x.jpg" },
            { kind: "image", path: "  " },
          ],
        },
      ],
    },
  ]);
  assert.deepEqual(msgs[0]!.activities?.[0]?.media, [
    { kind: "image", path: "ok.jpg" },
  ]);
});

test("coalesceConsecutiveAssistants merges same-turn assistant bubbles", () => {
  const merged = coalesceConsecutiveAssistants([
    { id: "u1", role: "user", content: "做首歌" },
    {
      id: "a1",
      role: "assistant",
      content: "先生成",
      activities: [{ id: "c1", kind: "tool", title: "music_gen", status: "done" }],
      segments: [
        { type: "reasoning", id: "r1", text: "t1", at: 1000 },
        { type: "activity", id: "c1", at: 2000 },
      ],
    },
    {
      id: "a2",
      role: "assistant",
      content: "生成成功",
      activities: [{ id: "c2", kind: "tool", title: "present_result", status: "done" }],
      segments: [
        { type: "reasoning", id: "r1", text: "t1", at: 1000 },
        { type: "activity", id: "c1", at: 2000 },
        { type: "reasoning", id: "r2", text: "t2", at: 5000 },
        { type: "activity", id: "c2", at: 6000 },
        { type: "surface", id: "surf-1", at: 6100 },
      ],
      uiSurfaces: [
        {
          messageId: "surf-1",
          activityType: "a2ui-surface",
          operations: [],
          status: "active",
        },
      ],
    },
    {
      id: "a3",
      role: "assistant",
      content: "搞定",
      segments: [
        { type: "reasoning", id: "r1", text: "t1", at: 1000 },
        { type: "activity", id: "c1", at: 2000 },
        { type: "reasoning", id: "r2", text: "t2", at: 5000 },
        { type: "activity", id: "c2", at: 6000 },
        { type: "surface", id: "surf-1", at: 6100 },
        { type: "reasoning", id: "r3", text: "t3", at: 7000 },
      ],
    },
  ]);
  assert.equal(merged.length, 2);
  assert.equal(merged[1]!.role, "assistant");
  assert.equal(merged[1]!.content, "先生成\n\n生成成功\n\n搞定");
  assert.equal(merged[1]!.activities?.length, 2);
  assert.equal(merged[1]!.segments?.length, 6);
  assert.equal(merged[1]!.uiSurfaces?.length, 1);
});

test("mapHistoryMessages restores durationSec from segment timestamps", () => {
  const msgs = mapHistoryMessages([
    {
      id: "a1",
      role: "assistant",
      content: "ok",
      activities: [
        { id: "c1", kind: "tool", title: "music_gen", status: "done" },
      ],
      segments: [
        { type: "reasoning", id: "r1", text: "think", at: 1000 },
        { type: "activity", id: "c1", at: 3500 },
        { type: "reasoning", id: "r2", text: "more", at: 5000 },
      ],
    },
  ]);
  assert.equal(msgs.length, 1);
  const segs = msgs[0]!.segments!;
  assert.equal(segs[0]!.type, "reasoning");
  if (segs[0]!.type === "reasoning") {
    assert.equal(segs[0]!.durationSec, 2.5);
  }
  assert.equal(msgs[0]!.reasoningDurationSec, 2.5);
  assert.equal(msgs[0]!.activities?.[0]?.durationSec, 1.5);
});

test("enrichTimelineDurations keeps existing durationSec", () => {
  const out = enrichTimelineDurations([
    { type: "reasoning", id: "r1", text: "a", at: 0, durationSec: 9 },
    { type: "activity", id: "c1", at: 1000 },
  ]);
  assert.equal(out[0]!.type, "reasoning");
  if (out[0]!.type === "reasoning") {
    assert.equal(out[0]!.durationSec, 9);
  }
});
