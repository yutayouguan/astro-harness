import type { ProviderDto } from "../../types";

export function providerRequiresApiKey(provider: ProviderDto): boolean {
  return provider.kind !== "ollama" && provider.key_source !== "not_required";
}

/** Configuration readiness, not a claim of successful live connectivity. */
export function providerIsReady(
  provider: ProviderDto | null | undefined,
): provider is ProviderDto {
  return Boolean(
    provider?.enabled &&
      provider.supports_responses_api === true &&
      provider.model.trim() &&
      (!providerRequiresApiKey(provider) || provider.has_api_key),
  );
}
