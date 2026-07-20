import assert from "node:assert/strict";
import { test } from "node:test";
import {
  EMPTY_MODEL_CAPABILITIES,
  formatKnowledgeCutoff,
  formatModelPrice,
  inferModelCapabilities,
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

test("inferModelCapabilities marks deepseek-v4 as reasoning", () => {
  const caps = inferModelCapabilities("deepseek-v4-flash", "deepseek");
  assert.equal(caps.reasoning, true);
  assert.equal(caps.tools, true);
});