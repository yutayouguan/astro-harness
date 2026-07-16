import assert from "node:assert/strict";
import { test } from "node:test";
import {
  EMPTY_MODEL_CAPABILITIES,
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
    }),
    ["tools", "reasoning", "web", "image_gen", "audio_gen"],
  );
});
