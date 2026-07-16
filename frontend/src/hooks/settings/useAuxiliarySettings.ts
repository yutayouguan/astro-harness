import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type {
  AuxiliarySettingsDto,
  AuxiliaryTaskId,
} from "../../types";

type UseAuxiliarySettings = {
  loading: boolean;
  error: string | null;
  settings: AuxiliarySettingsDto | null;
  setRoute(task: AuxiliaryTaskId, provider: string, model: string): Promise<void>;
  resetRoute(task: AuxiliaryTaskId): Promise<void>;
  resetAll(): Promise<void>;
  reload(): Promise<void>;
};

function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

export function useAuxiliarySettings(active = true): UseAuxiliarySettings {
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [settings, setSettings] = useState<AuxiliarySettingsDto | null>(null);

  const reload = useCallback(async () => {
    if (!active) return;
    setLoading(true);
    setError(null);
    try {
      const next = await invoke<AuxiliarySettingsDto>("get_auxiliary_settings");
      setSettings(next);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setLoading(false);
    }
  }, [active]);

  useEffect(() => {
    void reload();
  }, [reload]);

  const setRoute = useCallback(
    async (task: AuxiliaryTaskId, provider: string, model: string) => {
      setError(null);
      try {
        const next = await invoke<AuxiliarySettingsDto>("set_auxiliary_route", {
          task,
          provider,
          model,
        });
        setSettings(next);
      } catch (err) {
        setError(errorMessage(err));
      }
    },
    [],
  );

  const resetRoute = useCallback(async (task: AuxiliaryTaskId) => {
    setError(null);
    try {
      const next = await invoke<AuxiliarySettingsDto>("reset_auxiliary_route", {
        task,
      });
      setSettings(next);
    } catch (err) {
      setError(errorMessage(err));
    }
  }, []);

  const resetAll = useCallback(async () => {
    setError(null);
    try {
      const next = await invoke<AuxiliarySettingsDto>("reset_all_auxiliary_routes");
      setSettings(next);
    } catch (err) {
      setError(errorMessage(err));
    }
  }, []);

  return { loading, error, settings, setRoute, resetRoute, resetAll, reload };
}
