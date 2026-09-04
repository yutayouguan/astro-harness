import assert from "node:assert/strict";
import test from "node:test";
import {
  matchesModelMarketFilter,
  matchesModelMarketType,
  providerSaveInputWithEmbedding,
  type ModelCatalogEntry,
} from "./modelMarket.ts";

function model(overrides: Partial<ModelCatalogEntry> = {}): ModelCatalogEntry {
  return {
    id: "example/generator",
    name: "Generator",
    description: null,
    context_length: 32_000,
    created: null,
    pricing: null,
    model_type: "generation",
    supports_vision: false,
    supports_function_calling: false,
    supports_reasoning: false,
    supports_web_search: false,
    supports_image_generation: false,
    supports_video_generation: false,
    supports_audio_input: false,
    supports_audio_output: false,
    supports_transcription: false,
    supports_music_generation: false,
    input_modalities: ["text"],
    output_modalities: ["text"],
    knowledge_cutoff: null,
    expiration_date: null,
    ...overrides,
  };
}

test("model type filters use the structured model type", () => {
  const embedding = model({
    id: "example/embed",
    model_type: "embedding",
    output_modalities: ["embeddings"],
  });
  const reranker = model({
    id: "example/reranker",
    model_type: "rerank",
    output_modalities: ["rerank"],
  });

  assert.equal(matchesModelMarketType(embedding, "embedding"), true);
  assert.equal(matchesModelMarketType(embedding, "rerank"), false);
  assert.equal(matchesModelMarketType(reranker, "rerank"), true);
  assert.equal(matchesModelMarketType(reranker, "embedding"), false);
});

test("specialized filters do not classify models from names", () => {
  const misleading = model({ id: "example/not-an-embedding-model" });

  assert.equal(matchesModelMarketType(misleading, "embedding"), false);
  assert.equal(matchesModelMarketType(misleading, "rerank"), false);
});

test("media capability filters remain distinct", () => {
  const modelWithMedia = model({
    supports_image_generation: true,
    supports_video_generation: true,
    supports_audio_output: true,
    supports_transcription: true,
    supports_music_generation: true,
  });

  assert.equal(matchesModelMarketFilter(modelWithMedia, "image"), true);
  assert.equal(matchesModelMarketFilter(modelWithMedia, "video"), true);
  assert.equal(matchesModelMarketFilter(modelWithMedia, "speech"), true);
  assert.equal(matchesModelMarketFilter(modelWithMedia, "transcription"), true);
  assert.equal(matchesModelMarketFilter(modelWithMedia, "music"), true);

  const inputOnlyAudio = model({ supports_audio_input: true });
  assert.equal(matchesModelMarketFilter(inputOnlyAudio, "speech"), false);
  assert.equal(matchesModelMarketFilter(inputOnlyAudio, "transcription"), false);
});

test("embedding setup preserves the provider contract", () => {
  const input = providerSaveInputWithEmbedding(
    {
      id: "openrouter",
      kind: "openrouter",
      display_name: "OpenRouter",
      endpoint: "https://openrouter.ai/api/v1",
      model: "openai/gpt-5.6",
      enabled: true,
      has_api_key: true,
      key_source: "keyring",
      env_key_name: null,
      backend_id: "openrouter",
      fallback: [{ provider_id: "openai", model: "gpt-5.6" }],
      image_model: "image-model",
      video_model: "video-model",
      tts_model: "tts-model",
      vision_model: "vision-model",
      music_model: "music-model",
      embedding_model: "old-embedding",
    },
    " voyageai/voyage-4 ",
  );

  assert.equal(input.embedding_model, "voyageai/voyage-4");
  assert.equal(input.model, "openai/gpt-5.6");
  assert.deepEqual(input.fallback, [
    { provider_id: "openai", model: "gpt-5.6" },
  ]);
  assert.equal(input.image_model, "image-model");
});
