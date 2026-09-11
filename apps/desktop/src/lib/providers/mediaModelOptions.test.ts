import assert from "node:assert/strict";
import { test } from "node:test";
import type { ModelInfo } from "../../types.ts";
import { inferModelCapabilities } from "../model/modelCaps.ts";
import {
  buildMediaModelOptions,
  evaluateMediaModelsResult,
  filterModelsByCapability,
  isMediaModelsRequestLoading,
  MEDIA_CAPABILITY_BY_FIELD,
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
    file: false,
    audio_in: false,
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

test("both GPT Image 2.5 models remain selectable as media models", () => {
  const ids = ["gpt-image-2.5-flare", "gpt-image-2.5-sunburst"];
  const options = buildMediaModelOptions(
    ids.map((id) => ({
      id,
      capabilities: inferModelCapabilities(id, "azure"),
    })),
    "image_gen",
    "gpt-image-2",
  );
  for (const id of ids) assert.equal(sanitizeMediaModelValue(id, options), id);
  assert.equal(options[0]?.modelId, "gpt-image-2");
});

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

test("maps every media field to its independent capability", () => {
  assert.deepEqual(MEDIA_CAPABILITY_BY_FIELD, {
    image_model: "image_gen",
    video_model: "video_gen",
    tts_model: "audio_gen",
    music_model: "music_gen",
    vision_model: "vision",
  });
});

test("rejects a previous provider result after switching providers", () => {
  assert.deepEqual(
    evaluateMediaModelsResult(
      "provider-b",
      2,
      "provider-a",
      1,
      "online-success",
    ),
    { accept: false, showModels: false, sanitize: false },
  );
});

test("rejects an older request that resolves after a newer request", () => {
  assert.deepEqual(
    evaluateMediaModelsResult(
      "provider-a",
      2,
      "provider-a",
      1,
      "online-success",
    ),
    { accept: false, showModels: false, sanitize: false },
  );
});

test("shows current cache without allowing it to sanitize saved values", () => {
  assert.deepEqual(
    evaluateMediaModelsResult("provider-a", 1, "provider-a", 1, "cache"),
    { accept: true, showModels: true, sanitize: false },
  );
});

test("preserves cached models and saved values when online refresh fails", () => {
  assert.deepEqual(
    evaluateMediaModelsResult(
      "provider-a",
      1,
      "provider-a",
      1,
      "online-failure",
    ),
    { accept: true, showModels: false, sanitize: false },
  );
});

test("sanitizes only after the current online refresh succeeds", () => {
  assert.deepEqual(
    evaluateMediaModelsResult(
      "provider-a",
      1,
      "provider-a",
      1,
      "online-success",
    ),
    { accept: true, showModels: true, sanitize: true },
  );
});

test("stops showing an invalidated provider request as loading", () => {
  assert.equal(
    isMediaModelsRequestLoading("provider-b", 2, {
      providerId: "provider-a",
      requestId: 1,
    }),
    false,
  );
});

test("does not let an older request own loading for a newer request", () => {
  assert.equal(
    isMediaModelsRequestLoading("provider-a", 2, {
      providerId: "provider-a",
      requestId: 1,
    }),
    false,
  );
});

test("shows loading only for the current provider request", () => {
  assert.equal(
    isMediaModelsRequestLoading("provider-a", 2, {
      providerId: "provider-a",
      requestId: 2,
    }),
    true,
  );
});
