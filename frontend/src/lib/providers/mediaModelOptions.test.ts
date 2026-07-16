import assert from "node:assert/strict";
import { test } from "node:test";
import type { ModelInfo } from "../../types.ts";
import {
  buildMediaModelOptions,
  filterModelsByCapability,
  sanitizeMediaModelValue,
} from "./mediaModelOptions.ts";

const model = (
  id: string,
  caps: Partial<ModelInfo["capabilities"]>,
): ModelInfo => ({
  id,
  capabilities: {
    vision: false,
    web: false,
    reasoning: false,
    tools: false,
    image_gen: false,
    video_gen: false,
    audio_gen: false,
    music_gen: false,
    ...caps,
  },
});

const models = [
  model("image-a", { image_gen: true }),
  model("tts-a", { audio_gen: true }),
  model("music-a", { music_gen: true }),
  model("unknown-a", {}),
];

test("filters strictly by requested capability", () => {
  assert.deepEqual(
    filterModelsByCapability(models, "music_gen").map((m) => m.id),
    ["music-a"],
  );
});

test("always prepends the empty built-in default and hides unknown models", () => {
  assert.deepEqual(
    buildMediaModelOptions(models, "image_gen", "default-image"),
    [
      { value: "", modelId: "default-image" },
      { value: "image-a", modelId: "image-a" },
    ],
  );
});

test("clears saved values absent from the filtered options", () => {
  const options = buildMediaModelOptions(models, "music_gen", "default-music");
  assert.equal(sanitizeMediaModelValue("unknown-a", options), "");
  assert.equal(sanitizeMediaModelValue("music-a", options), "music-a");
  assert.equal(sanitizeMediaModelValue("", options), "");
});
