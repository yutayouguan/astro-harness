import assert from "node:assert/strict";
import test from "node:test";
import {
  matchesModelMarketFilter,
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
    supports_audio_input: false,
    supports_audio_output: false,
    input_modalities: ["text"],
    output_modalities: ["text"],
    knowledge_cutoff: null,
    expiration_date: null,
    ...overrides,
  };
}

test("embedding and rerank filters use the structured model type", () => {
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

  assert.equal(matchesModelMarketFilter(embedding, "embedding"), true);
  assert.equal(matchesModelMarketFilter(embedding, "rerank"), false);
  assert.equal(matchesModelMarketFilter(reranker, "rerank"), true);
  assert.equal(matchesModelMarketFilter(reranker, "embedding"), false);
});

test("specialized filters do not classify models from names", () => {
  const misleading = model({ id: "example/not-an-embedding-model" });

  assert.equal(matchesModelMarketFilter(misleading, "embedding"), false);
  assert.equal(matchesModelMarketFilter(misleading, "rerank"), false);
});
