import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { ProviderDto, ProvidersStateDto } from "../../types";

export function useProviders() {
  const [providers, setProviders] = useState<ProviderDto[]>([]);
  const [activeProviderId, setActiveProviderId] = useState<string | null>(null);
  const providersRef = useRef(providers);
  providersRef.current = providers;

  const syncProvidersFromState = useCallback((state: ProvidersStateDto) => {
    const enabled = state.providers.filter((p) => p.enabled);
    setProviders(enabled);
    const activeId =
      state.active_provider_id &&
      enabled.some((p) => p.id === state.active_provider_id)
        ? state.active_provider_id
        : (enabled[0]?.id ?? null);
    setActiveProviderId(activeId);
  }, []);

  useEffect(() => {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
      return;
    }
    invoke<ProvidersStateDto>("get_providers_state")
      .then(syncProvidersFromState)
      .catch(() =>
        invoke<ProviderDto[]>("list_providers")
          .then((list) => {
            setProviders(list);
            if (list[0]) setActiveProviderId(list[0].id);
          })
          .catch(() => {}),
      );
  }, [syncProvidersFromState]);

  const onChatModelChange = useCallback(
    async (providerId: string, model: string) => {
      const provider = providersRef.current.find((p) => p.id === providerId);
      if (!provider) return;

      setProviders((prev) =>
        prev.map((p) => (p.id === providerId ? { ...p, model } : p)),
      );
      setActiveProviderId(providerId);

      try {
        if (provider.model !== model) {
          await invoke<ProvidersStateDto>("save_provider", {
            provider: {
              id: provider.id,
              kind: provider.kind,
              display_name: provider.display_name,
              endpoint: provider.endpoint,
              model,
              enabled: provider.enabled,
            },
          });
        }
        const next = await invoke<ProvidersStateDto>("set_active_provider", {
          id: providerId,
        });
        syncProvidersFromState(next);
      } catch {
        // optimistic update stands on failure
      }
    },
    [syncProvidersFromState],
  );

  const activeProvider =
    providers.find((p) => p.id === activeProviderId) ?? providers[0];

  return {
    providers,
    activeProviderId,
    activeProvider,
    syncProvidersFromState,
    onChatModelChange,
  };
}
