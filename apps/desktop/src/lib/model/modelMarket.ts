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
  supports_video_generation: boolean;
  supports_audio_input: boolean;
  supports_audio_output: boolean;
  supports_transcription: boolean;
  supports_music_generation: boolean;
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
  | "image"
  | "video"
  | "speech"
  | "transcription"
  | "music"
  | "free";

export type ModelMarketTypeFilter = "all" | ModelCatalogKind;

export function matchesModelMarketType(
  model: ModelCatalogEntry,
  type: ModelMarketTypeFilter,
): boolean {
  return type === "all" || model.model_type === type;
}

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
    case "image":
      return model.supports_image_generation;
    case "video":
      return model.supports_video_generation;
    case "speech":
      return model.supports_audio_output;
    case "transcription":
      return model.supports_transcription;
    case "music":
      return model.supports_music_generation;
    case "free":
      return (
        model.pricing?.prompt_per_million === 0 &&
        model.pricing?.completion_per_million === 0
      );
  }
}

export function providerSaveInputWithEmbedding(
  provider: ProviderDto,
  embeddingModel: string,
) {
  return {
    id: provider.id,
    kind: provider.kind,
    display_name: provider.display_name,
    endpoint: provider.endpoint,
    model: provider.model,
    enabled: provider.enabled,
    fallback: provider.fallback ?? [],
    image_model: provider.image_model?.trim() ?? "",
    video_model: provider.video_model?.trim() ?? "",
    tts_model: provider.tts_model?.trim() ?? "",
    vision_model: provider.vision_model?.trim() ?? "",
    music_model: provider.music_model?.trim() ?? "",
    embedding_model: embeddingModel.trim(),
  };
}
import type { ProviderDto } from "../../types";
