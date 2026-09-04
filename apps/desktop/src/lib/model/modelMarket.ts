export interface ModelPricing {
  prompt_per_million: number | null;
  completion_per_million: number | null;
  cache_read_per_million: number | null;
  cache_write_per_million: number | null;
}

export type ModelCatalogKind = "generation" | "embedding" | "rerank";

export interface ModelCatalogEntry {
  id: string;
  name: string | null;
  description: string | null;
  context_length: number | null;
  created: number | null;
  pricing: ModelPricing | null;
  model_type: ModelCatalogKind;
  supports_vision: boolean;
  supports_function_calling: boolean;
  supports_reasoning: boolean;
  supports_web_search: boolean;
  supports_image_generation: boolean;
  supports_audio_input: boolean;
  supports_audio_output: boolean;
  input_modalities: string[];
  output_modalities: string[];
  knowledge_cutoff: string | null;
  expiration_date: string | null;
}

export type ModelMarketFilter =
  | "all"
  | "tools"
  | "reasoning"
  | "vision"
  | "audio"
  | "image"
  | "embedding"
  | "rerank"
  | "free";

export function matchesModelMarketFilter(
  model: ModelCatalogEntry,
  filter: ModelMarketFilter,
): boolean {
  switch (filter) {
    case "all":
      return true;
    case "tools":
      return model.supports_function_calling;
    case "reasoning":
      return model.supports_reasoning;
    case "vision":
      return model.supports_vision;
    case "audio":
      return model.supports_audio_input || model.supports_audio_output;
    case "image":
      return model.supports_image_generation;
    case "embedding":
      return model.model_type === "embedding";
    case "rerank":
      return model.model_type === "rerank";
    case "free":
      return (
        model.pricing?.prompt_per_million === 0 &&
        model.pricing?.completion_per_million === 0
      );
  }
}
