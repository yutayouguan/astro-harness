import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type {
  EvolutionProposalDto,
  EvolutionRunReport,
  EvolutionSearchReport,
} from "../../types";

type UseEvolutionProposals = {
  loading: boolean;
  running: boolean;
  error: string | null;
  lastReport: EvolutionRunReport | null;
  lastSearch: EvolutionSearchReport | null;
  proposals: EvolutionProposalDto[];
  run(): Promise<void>;
  runSearch(): Promise<void>;
  approve(id: string): Promise<void>;
  approveToBranch(id: string): Promise<string | null>;
  reject(id: string): Promise<void>;
  reload(): Promise<void>;
};

function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

export function useEvolutionProposals(active = true): UseEvolutionProposals {
  const [loading, setLoading] = useState(false);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [lastReport, setLastReport] = useState<EvolutionRunReport | null>(null);
  const [lastSearch, setLastSearch] = useState<EvolutionSearchReport | null>(null);
  const [proposals, setProposals] = useState<EvolutionProposalDto[]>([]);

  const reload = useCallback(async () => {
    if (!active) return;
    setLoading(true);
    setError(null);
    try {
      const next = await invoke<EvolutionProposalDto[]>("list_evolution_proposals");
      setProposals(next);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setLoading(false);
    }
  }, [active]);

  useEffect(() => {
    void reload();
  }, [reload]);

  const run = useCallback(async () => {
    setRunning(true);
    setError(null);
    try {
      const report = await invoke<EvolutionRunReport>("run_evolution");
      setLastReport(report);
      await reload();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setRunning(false);
    }
  }, [reload]);

  const runSearch = useCallback(async () => {
    setRunning(true);
    setError(null);
    try {
      const report = await invoke<EvolutionSearchReport>("run_evolution_search");
      setLastSearch(report);
      await reload();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setRunning(false);
    }
  }, [reload]);

  const approve = useCallback(async (id: string) => {
    setError(null);
    try {
      await invoke<string>("approve_evolution_proposal", { id });
      setProposals((prev) => prev.filter((p) => p.id !== id));
    } catch (err) {
      setError(errorMessage(err));
    }
  }, []);

  const approveToBranch = useCallback(async (id: string): Promise<string | null> => {
    setError(null);
    try {
      const msg = await invoke<string>("approve_evolution_proposal_to_branch", { id });
      setProposals((prev) => prev.filter((p) => p.id !== id));
      return msg;
    } catch (err) {
      setError(errorMessage(err));
      return null;
    }
  }, []);

  const reject = useCallback(async (id: string) => {
    setError(null);
    try {
      await invoke("reject_evolution_proposal", { id });
      setProposals((prev) => prev.filter((p) => p.id !== id));
    } catch (err) {
      setError(errorMessage(err));
    }
  }, []);

  return {
    loading,
    running,
    error,
    lastReport,
    lastSearch,
    proposals,
    run,
    runSearch,
    approve,
    approveToBranch,
    reject,
    reload,
  };
}
