import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { CompressionSettingsDto } from "../../types";

type UseCompressionSettings = {
  loading: boolean;
  error: string | null;
  settings: CompressionSettingsDto | null;
  save(settings: CompressionSettingsDto): Promise<void>;
  reset(): Promise<void>;
  reload(): Promise<void>;
};

function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

export function useCompressionSettings(active = true): UseCompressionSettings {
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [settings, setSettings] = useState<CompressionSettingsDto | null>(null);

  const reload = useCallback(async () => {
    if (!active) return;
    setLoading(true);
    setError(null);
    try {
      const next = await invoke<CompressionSettingsDto>("get_compression_settings");
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

  const save = useCallback(async (next: CompressionSettingsDto) => {
    setError(null);
    try {
      const saved = await invoke<CompressionSettingsDto>("set_compression_settings", {
        settings: next,
      });
      setSettings(saved);
    } catch (err) {
      setError(errorMessage(err));
      throw err;
    }
  }, []);

  const reset = useCallback(async () => {
    setError(null);
    try {
      const next = await invoke<CompressionSettingsDto>("reset_compression_settings");
      setSettings(next);
    } catch (err) {
      setError(errorMessage(err));
    }
  }, []);

  return { loading, error, settings, save, reset, reload };
}
