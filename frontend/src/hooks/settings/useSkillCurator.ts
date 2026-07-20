import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { CurateReportDto, CuratorStatusDto } from "../../types";

type CuratorRunReport = {
  report: CurateReportDto;
  enqueued: number;
};

type UseSkillCurator = {
  loading: boolean;
  error: string | null;
  report: CurateReportDto | null;
  status: CuratorStatusDto | null;
  lastEnqueued: number;
  run(enqueue?: boolean): Promise<void>;
  enqueue(): Promise<void>;
  reload(): Promise<void>;
};

function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

export function useSkillCurator(active = true): UseSkillCurator {
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [report, setReport] = useState<CurateReportDto | null>(null);
  const [status, setStatus] = useState<CuratorStatusDto | null>(null);
  const [lastEnqueued, setLastEnqueued] = useState(0);

  const reload = useCallback(async () => {
    if (!active) return;
    try {
      const [last, st] = await Promise.all([
        invoke<CurateReportDto | null>("get_curator_last"),
        invoke<CuratorStatusDto>("curator_status"),
      ]);
      setReport(last);
      setStatus(st);
    } catch {
      // 无历史报告时忽略
    }
  }, [active]);

  useEffect(() => {
    void reload();
  }, [reload]);

  const run = useCallback(
    async (enqueue = false) => {
      setLoading(true);
      setError(null);
      try {
        const next = await invoke<CuratorRunReport>("run_skill_curator", { enqueue });
        setReport(next.report);
        setLastEnqueued(next.enqueued);
        const st = await invoke<CuratorStatusDto>("curator_status");
        setStatus(st);
      } catch (err) {
        setError(errorMessage(err));
      } finally {
        setLoading(false);
      }
    },
    [],
  );

  const enqueue = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const n = await invoke<number>("enqueue_curator_proposals");
      setLastEnqueued(n);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setLoading(false);
    }
  }, []);

  return { loading, error, report, status, lastEnqueued, run, enqueue, reload };
}
