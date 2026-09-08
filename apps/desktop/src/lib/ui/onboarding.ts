import type { ProviderDto } from "../../types";

export const ONBOARDING_VERSION = 1;
export const ONBOARDING_RESET_EVENT = "astro:onboarding-reset";
export const ONBOARDING_STARTER_PROMPT_KEY =
  "astro.onboarding.starterPrompt.v1";

export const ONBOARDING_STEPS = [
  "intro",
  "personalize",
  "provider",
  "workspace",
  "complete",
] as const;

export type OnboardingStep = (typeof ONBOARDING_STEPS)[number];

export type OnboardingStateDto = {
  version: number;
  step: string;
  completed: boolean;
  should_show: boolean;
  inferred_existing_install: boolean;
  updated_at: string | null;
};

export function normalizeOnboardingStep(step: string): OnboardingStep {
  return ONBOARDING_STEPS.includes(step as OnboardingStep)
    ? (step as OnboardingStep)
    : "intro";
}

export function providerRequiresApiKey(provider: ProviderDto): boolean {
  return provider.kind !== "ollama" && provider.key_source !== "not_required";
}

export function providerIsReady(provider: ProviderDto): boolean {
  return (
    provider.enabled &&
    provider.supports_responses_api === true &&
    (!providerRequiresApiKey(provider) || provider.has_api_key)
  );
}

export function providerConfigInput(provider: ProviderDto, model: string) {
  return {
    id: provider.id,
    kind: provider.kind,
    display_name: provider.display_name,
    endpoint: provider.endpoint,
    model: model.trim() || provider.model,
    enabled: true,
    fallback: provider.fallback ?? [],
    image_model: provider.image_model ?? "",
    video_model: provider.video_model ?? "",
    tts_model: provider.tts_model ?? "",
    music_model: provider.music_model ?? "",
    vision_model: provider.vision_model ?? "",
    embedding_model: provider.embedding_model ?? "",
  };
}

export function inferProjectName(path: string): string {
  const segments = path.split(/[\\/]/).filter(Boolean);
  return segments[segments.length - 1] ?? "Workspace";
}

export function storeOnboardingStarterPrompt(prompt: string): void {
  const value = prompt.trim();
  if (!value || typeof window === "undefined") return;
  try {
    window.sessionStorage.setItem(ONBOARDING_STARTER_PROMPT_KEY, value);
  } catch {
    // Session storage may be unavailable in restricted WebViews.
  }
}

export function takeOnboardingStarterPrompt(): string | null {
  if (typeof window === "undefined") return null;
  try {
    const value = window.sessionStorage.getItem(ONBOARDING_STARTER_PROMPT_KEY);
    window.sessionStorage.removeItem(ONBOARDING_STARTER_PROMPT_KEY);
    return value?.trim() || null;
  } catch {
    return null;
  }
}
