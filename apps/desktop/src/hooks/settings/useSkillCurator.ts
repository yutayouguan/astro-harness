import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { CurateReportDto, CuratorStatusDto } from "../../types";

type CuratorRunReport = {
  report: CurateReportDto;
  enqueued: number;
};

type CuratorUpdatedPayload = {
  suggestionCount?: number;
  auto?: boolean;
};

type UseSkillCurator = {
  loading: boolean;
  error: string | null;
  report: CurateReportDto | null;
  status: CuratorStatusDto | null;
  lastEnqueued: number;
  /** 后台自动刷新提示（展示后可 clear） */
  autoNotice: string | null;
  clearAutoNotice(): void;
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
  const [autoNotice, setAutoNotice] = useState<string | null>(null);

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

  // 后台到期策展完成后刷新报告 / 状态
  useEffect(() => {
    if (!active) return;
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window))
      return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<CuratorUpdatedPayload>("curator-updated", (ev) => {
      void reload();
      if (ev.payload?.auto) {
        const n = ev.payload.suggestionCount ?? 0;
        setAutoNotice(String(n));
      }
    })
      .then((fn) => {
        if (disposed) fn();
        else unlisten = fn;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [active, reload]);

  const clearAutoNotice = useCallback(() => setAutoNotice(null), []);

  const run = useCallback(async (enqueue = false) => {
    setLoading(true);
    setError(null);
    try {
      const next = await invoke<CuratorRunReport>("run_skill_curator", {
        enqueue,
      });
      setReport(next.report);
      setLastEnqueued(next.enqueued);
      const st = await invoke<CuratorStatusDto>("curator_status");
      setStatus(st);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setLoading(false);
    }
  }, []);

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

  return {
    loading,
    error,
    report,
    status,
    lastEnqueued,
    autoNotice,
    clearAutoNotice,
    run,
    enqueue,
    reload,
  };
}
