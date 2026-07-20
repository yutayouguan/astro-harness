import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { CurateReportDto } from "../../types";

type UseSkillCurator = {
  loading: boolean;
  error: string | null;
  report: CurateReportDto | null;
  run(): Promise<void>;
  reload(): Promise<void>;
};

function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

export function useSkillCurator(active = true): UseSkillCurator {
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [report, setReport] = useState<CurateReportDto | null>(null);

  const reload = useCallback(async () => {
    if (!active) return;
    try {
      const last = await invoke<CurateReportDto | null>("get_curator_last");
      setReport(last);
    } catch {
      // 无历史报告时忽略
    }
  }, [active]);

  useEffect(() => {
    void reload();
  }, [reload]);

  const run = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const next = await invoke<CurateReportDto>("run_skill_curator");
      setReport(next);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setLoading(false);
    }
  }, []);

  return { loading, error, report, run, reload };
}
