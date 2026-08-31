import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { EvolutionAutoStatusDto } from "../../types";

type UseEvolutionAuto = {
  status: EvolutionAutoStatusDto | null;
  error: string | null;
  reload(): Promise<void>;
};

function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

/** 读取自动触发护栏水位（配置变更 / Chat Done 后可手动刷新）。 */
export function useEvolutionAuto(active = true): UseEvolutionAuto {
  const [status, setStatus] = useState<EvolutionAutoStatusDto | null>(null);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    if (!active) return;
    setError(null);
    try {
      const next = await invoke<EvolutionAutoStatusDto>(
        "evolution_auto_status",
      );
      setStatus(next);
    } catch (err) {
      setError(errorMessage(err));
    }
  }, [active]);

  useEffect(() => {
    void reload();
  }, [reload]);

  return { status, error, reload };
}
