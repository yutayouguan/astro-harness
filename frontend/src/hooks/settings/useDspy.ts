import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { DspyStatusDto, EvolutionRunReport } from "../../types";

type UseDspy = {
  loading: boolean;
  busy: boolean;
  error: string | null;
  message: string | null;
  status: DspyStatusDto | null;
  reload(): Promise<void>;
  setup(): Promise<void>;
  run(skillId: string, mock?: boolean): Promise<EvolutionRunReport | null>;
};

function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

export function useDspy(active = true): UseDspy {
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [status, setStatus] = useState<DspyStatusDto | null>(null);

  const reload = useCallback(async () => {
    if (!active) return;
    setLoading(true);
    setError(null);
    try {
      const next = await invoke<DspyStatusDto>("evolution_dspy_status");
      setStatus(next);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setLoading(false);
    }
  }, [active]);

  useEffect(() => {
    void reload();
  }, [reload]);

  const setup = useCallback(async () => {
    setBusy(true);
    setError(null);
    setMessage(null);
    try {
      const msg = await invoke<string>("setup_evolution_dspy");
      setMessage(msg);
      await reload();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }, [reload]);

  const run = useCallback(
    async (skillId: string, mock = false): Promise<EvolutionRunReport | null> => {
    setBusy(true);
    setError(null);
    setMessage(null);
    try {
      const report = await invoke<EvolutionRunReport>("run_evolution_dspy", { skillId, mock });
      setMessage(
        report.proposals.length > 0
          ? `已生成 ${report.proposals.length} 条待审提案`
          : "运行完成，未产出提案",
      );
      return report;
    } catch (err) {
      setError(errorMessage(err));
      return null;
    } finally {
      setBusy(false);
    }
    },
    [],
  );

  return { loading, busy, error, message, status, reload, setup, run };
}
