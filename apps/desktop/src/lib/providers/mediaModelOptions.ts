import type { ModelInfo } from "../../types.ts";

export type MediaCapabilityKey =
  | "image_gen"
  | "video_gen"
  | "audio_gen"
  | "music_gen"
  | "vision";

export type MediaModelField =
  | "image_model"
  | "video_model"
  | "tts_model"
  | "music_model"
  | "vision_model";

export type MediaModelOption = {
  value: string;
  modelId: string;
};

export type MediaModelsResultKind =
  | "cache"
  | "online-success"
  | "online-failure";

export type MediaModelsResultDecision = {
  accept: boolean;
  showModels: boolean;
  sanitize: boolean;
};

export type MediaModelsLoadingRequest = {
  providerId: string;
  requestId: number;
};

export function isMediaModelsRequestLoading(
  activeProviderId: string | null,
  activeRequestId: number,
  loadingRequest: MediaModelsLoadingRequest | null,
): boolean {
  return (
    loadingRequest?.providerId === activeProviderId &&
    loadingRequest.requestId === activeRequestId
  );
}

export function evaluateMediaModelsResult(
  activeProviderId: string | null,
  activeRequestId: number,
  resultProviderId: string,
  resultRequestId: number,
  kind: MediaModelsResultKind,
): MediaModelsResultDecision {
  const accept =
    activeProviderId === resultProviderId &&
    activeRequestId === resultRequestId;
  return {
    accept,
    showModels: accept && kind !== "online-failure",
    sanitize: accept && kind === "online-success",
  };
}

export const MEDIA_CAPABILITY_BY_FIELD: Record<
  MediaModelField,
  MediaCapabilityKey
> = {
  image_model: "image_gen",
  video_model: "video_gen",
  tts_model: "audio_gen",
  music_model: "music_gen",
  vision_model: "vision",
};

export function filterModelsByCapability(
  models: ModelInfo[],
  capability: MediaCapabilityKey,
): ModelInfo[] {
  return models.filter((model) => model.capabilities?.[capability] === true);
}

export function buildMediaModelOptions(
  models: ModelInfo[],
  capability: MediaCapabilityKey,
  defaultModelId: string,
): MediaModelOption[] {
  return [
    { value: "", modelId: defaultModelId },
    ...filterModelsByCapability(models, capability).map((model) => ({
      value: model.id,
      modelId: model.id,
    })),
  ];
}

export function sanitizeMediaModelValue(
  value: string,
  options: MediaModelOption[],
): string {
  const trimmed = value.trim();
  if (!trimmed) return "";
  return options.some((option) => option.value === trimmed) ? trimmed : "";
}
