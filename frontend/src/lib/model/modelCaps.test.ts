import assert from "node:assert/strict";
import { test } from "node:test";
import {
  EMPTY_MODEL_CAPABILITIES,
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
      web: true,
      image_gen: true,
      video_gen: false,
      audio_gen: true,
      music_gen: true,
    }),
    ["tools", "reasoning", "web", "image_gen", "audio_gen", "music_gen"],
  );
});

test("inferModelCapabilities marks deepseek-v4 as reasoning", () => {
  const caps = inferModelCapabilities("deepseek-v4-flash", "deepseek");
  assert.equal(caps.reasoning, true);
  assert.equal(caps.tools, true);
});