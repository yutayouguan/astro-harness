import assert from "node:assert/strict";
import { test } from "node:test";
import {
  EMPTY_MODEL_CAPABILITIES,
  attachmentAcceptForCaps,
  attachmentKindAllowed,
  compareModelsByCreatedDesc,
  estimateTurnCostUsd,
  formatEstimateCostUsd,
  formatKnowledgeCutoff,
  formatModelPrice,
  inferModelCapabilities,
  isModelCreatedWithinDays,
  listActiveModelCaps,
} from "./modelCaps.ts";

test("listActiveModelCaps returns empty for none/undefined", () => {
  assert.deepEqual(listActiveModelCaps(undefined), []);
  assert.deepEqual(listActiveModelCaps(EMPTY_MODEL_CAPABILITIES), []);
});

test("listActiveModelCaps keeps fixed order", () => {
  assert.deepEqual(
    listActiveModelCaps({
      tools: true,
      reasoning: true,
      vision: false,
      file: true,
      audio_in: false,
      web: true,
      image_gen: true,
      video_gen: false,
      audio_gen: true,
      music_gen: true,
    }),
    ["tools", "reasoning", "file", "web", "image_gen", "audio_gen", "music_gen"],
  );
});

test("formatModelPrice formats per-million USD", () => {
  assert.equal(
    formatModelPrice({
      prompt_per_million: 0.14,
      completion_per_million: 0.28,
    }),
    "$0.14/$0.28",
  );
  assert.equal(formatModelPrice(null), null);
});

test("formatKnowledgeCutoff prefers YYYY-MM", () => {
  assert.equal(formatKnowledgeCutoff("2024-10-01"), "2024-10");
  assert.equal(formatKnowledgeCutoff(null), null);
});

test("isModelCreatedWithinDays / compareModelsByCreatedDesc", () => {
  const now = Date.parse("2026-07-20T12:00:00Z");
  const threeDaysAgo = Math.floor((now - 3 * 86400_000) / 1000);
  const tenDaysAgo = Math.floor((now - 10 * 86400_000) / 1000);
  assert.equal(isModelCreatedWithinDays(threeDaysAgo, 7, now), true);
  assert.equal(isModelCreatedWithinDays(tenDaysAgo, 7, now), false);
  assert.equal(isModelCreatedWithinDays(null, 7, now), false);

  const sorted = [
    { id: "old", created: tenDaysAgo },
    { id: "new", created: threeDaysAgo },
    { id: "none", created: null },
  ].sort(compareModelsByCreatedDesc);
  assert.deepEqual(
    sorted.map((m) => m.id),
    ["new", "old", "none"],
  );
  // 均无 created 时保持相对顺序（稳定排序）
  const noCreated = [
    { id: "b", created: null },
    { id: "a", created: null },
  ].sort(compareModelsByCreatedDesc);
  assert.deepEqual(
    noCreated.map((m) => m.id),
    ["b", "a"],
  );
});

test("attachmentAcceptForCaps gates by capabilities", () => {
  assert.equal(
    attachmentAcceptForCaps({
      ...EMPTY_MODEL_CAPABILITIES,
      vision: true,
    }),
    "image/*,video/*",
  );
  assert.equal(attachmentKindAllowed("image", { ...EMPTY_MODEL_CAPABILITIES }), false);
  assert.equal(
    attachmentKindAllowed("audio", {
      ...EMPTY_MODEL_CAPABILITIES,
      audio_in: true,
    }),
    true,
  );
  assert.ok(attachmentAcceptForCaps(null).includes("image/*"));
});

test("estimateTurnCostUsd uses per-million pricing", () => {
  const usd = estimateTurnCostUsd({
    pricing: { prompt_per_million: 1, completion_per_million: 2 },
    inputChars: 4000,
    expectedOutputTokens: 500,
  });
  // 1000 in * $1/M + 500 out * $2/M = 0.001 + 0.001
  assert.equal(usd, 0.002);
  assert.equal(formatEstimateCostUsd(0.002), "~$0.002");
});

test("inferModelCapabilities marks deepseek-v4 as reasoning", () => {
  const caps = inferModelCapabilities("deepseek-v4-flash", "deepseek");
  assert.equal(caps.reasoning, true);
  assert.equal(caps.tools, true);
});
