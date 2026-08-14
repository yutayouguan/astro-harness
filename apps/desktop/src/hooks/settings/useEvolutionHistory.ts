import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { EvolutionHistoryDto } from "../../types";

type UseEvolutionHistory = {
  loading: boolean;
  error: string | null;
  history: EvolutionHistoryDto | null;
  reload(): Promise<void>;
};

function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

export function useEvolutionHistory(active = true): UseEvolutionHistory {
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [history, setHistory] = useState<EvolutionHistoryDto | null>(null);

  const reload = useCallback(async () => {
    if (!active) return;
    setLoading(true);
    setError(null);
    try {
      const next = await invoke<EvolutionHistoryDto>("evolution_history");
      setHistory(next);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setLoading(false);
    }
  }, [active]);

  useEffect(() => {
    void reload();
  }, [reload]);

  return { loading, error, history, reload };
}
