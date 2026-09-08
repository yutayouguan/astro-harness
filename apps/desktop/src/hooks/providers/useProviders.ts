import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { ProviderDto, ProvidersStateDto } from "../../types";

export function useProviders() {
  const [providers, setProviders] = useState<ProviderDto[]>([]);
  const [activeProviderId, setActiveProviderId] = useState<string | null>(null);
  const providersRef = useRef(providers);
  const selectionRevisionRef = useRef(0);
  const providerMutationRef = useRef<Promise<void>>(Promise.resolve());
  providersRef.current = providers;

  const syncProvidersFromState = useCallback((state: ProvidersStateDto) => {
    const enabled = state.providers.filter(
      (p) => p.enabled && p.supports_responses_api === true,
    );
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
    let disposed = false;
    invoke<ProvidersStateDto>("get_providers_state")
      .then((state) => {
        if (!disposed) syncProvidersFromState(state);
      })
      .catch((error) => {
        console.warn("provider state load failed", error);
        invoke<ProviderDto[]>("list_providers")
          .then((list) => {
            if (disposed) return;
            const responsesProviders = list.filter(
              (provider) => provider.supports_responses_api === true,
            );
            setProviders(responsesProviders);
            if (responsesProviders[0])
              setActiveProviderId(responsesProviders[0].id);
          })
          .catch((fallbackError) => {
            console.warn("provider list fallback failed", fallbackError);
          });
      });
    return () => {
      disposed = true;
    };
  }, [syncProvidersFromState]);

  const onChatModelChange = useCallback(
    async (providerId: string, model: string) => {
      const provider = providersRef.current.find((p) => p.id === providerId);
      if (!provider) return;

      setProviders((prev) =>
        prev.map((p) => (p.id === providerId ? { ...p, model } : p)),
      );
      setActiveProviderId(providerId);

      const revision = ++selectionRevisionRef.current;
      let nextState: ProvidersStateDto | null = null;
      const mutation = providerMutationRef.current
        .catch(() => undefined)
        .then(async () => {
          nextState = await invoke<ProvidersStateDto>(
            "set_active_provider_model",
            { id: providerId, model },
          );
        });
      providerMutationRef.current = mutation.then(
        () => undefined,
        () => undefined,
      );
      try {
        await mutation;
        if (selectionRevisionRef.current === revision && nextState) {
          syncProvidersFromState(nextState);
        }
      } catch (error) {
        if (selectionRevisionRef.current !== revision) return;
        console.warn("provider selection failed", error);
        void invoke<ProvidersStateDto>("get_providers_state")
          .then(syncProvidersFromState)
          .catch((reloadError) => {
            console.warn("provider state reload failed", reloadError);
          });
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
