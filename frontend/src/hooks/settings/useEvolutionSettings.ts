import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { EvolutionRouteId, EvolutionSettingsDto } from "../../types";

type UseEvolutionSettings = {
  loading: boolean;
  error: string | null;
  settings: EvolutionSettingsDto | null;
  setEnabled(enabled: boolean): Promise<void>;
  setRoute(route: EvolutionRouteId, provider: string, model: string): Promise<void>;
  resetRoute(route: EvolutionRouteId): Promise<void>;
  setGates(
    runTests: boolean,
    maxSkillBytes: number,
    requirePr: boolean,
    minJudgeScore: number,
  ): Promise<void>;
  setSearch(
    generations: number,
    variants: number,
    crossover: boolean,
    populationSize?: number,
    maxEvalExamples?: number,
    maxLlmCalls?: number,
  ): Promise<void>;
  setAuto(
    enabled: boolean,
    cooldownSecs: number,
    minNewDecisions: number,
    maxRunsPerDay: number,
  ): Promise<void>;
  reload(): Promise<void>;
};

function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

export function useEvolutionSettings(active = true): UseEvolutionSettings {
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [settings, setSettings] = useState<EvolutionSettingsDto | null>(null);

  const reload = useCallback(async () => {
    if (!active) return;
    setLoading(true);
    setError(null);
    try {
      const next = await invoke<EvolutionSettingsDto>("get_evolution_settings");
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

  const setEnabled = useCallback(async (enabled: boolean) => {
    setError(null);
    try {
      const next = await invoke<EvolutionSettingsDto>("set_evolution_enabled", { enabled });
      setSettings(next);
    } catch (err) {
      setError(errorMessage(err));
    }
  }, []);

  const setRoute = useCallback(
    async (route: EvolutionRouteId, provider: string, model: string) => {
      setError(null);
      try {
        const next = await invoke<EvolutionSettingsDto>("set_evolution_route", {
          route,
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

  const resetRoute = useCallback(async (route: EvolutionRouteId) => {
    setError(null);
    try {
      const next = await invoke<EvolutionSettingsDto>("reset_evolution_route", { route });
      setSettings(next);
    } catch (err) {
      setError(errorMessage(err));
    }
  }, []);

  const setGates = useCallback(
    async (
      runTests: boolean,
      maxSkillBytes: number,
      requirePr: boolean,
      minJudgeScore: number,
    ) => {
      setError(null);
      try {
        const next = await invoke<EvolutionSettingsDto>("set_evolution_gates", {
          runTests,
          maxSkillBytes,
          requirePr,
          minJudgeScore,
        });
        setSettings(next);
      } catch (err) {
        setError(errorMessage(err));
      }
    },
    [],
  );

  const setSearch = useCallback(
    async (
      generations: number,
      variants: number,
      crossover: boolean,
      populationSize?: number,
      maxEvalExamples?: number,
      maxLlmCalls?: number,
    ) => {
      setError(null);
      try {
        const next = await invoke<EvolutionSettingsDto>("set_evolution_search", {
          generations,
          variants,
          crossover,
          populationSize: populationSize ?? null,
          maxEvalExamples: maxEvalExamples ?? null,
          maxLlmCalls: maxLlmCalls ?? null,
        });
        setSettings(next);
      } catch (err) {
        setError(errorMessage(err));
      }
    },
    [],
  );

  const setAuto = useCallback(
    async (
      enabled: boolean,
      cooldownSecs: number,
      minNewDecisions: number,
      maxRunsPerDay: number,
    ) => {
      setError(null);
      try {
        const next = await invoke<EvolutionSettingsDto>("set_evolution_auto", {
          enabled,
          cooldownSecs,
          minNewDecisions,
          maxRunsPerDay,
        });
        setSettings(next);
      } catch (err) {
        setError(errorMessage(err));
      }
    },
    [],
  );

  return {
    loading,
    error,
    settings,
    setEnabled,
    setRoute,
    resetRoute,
    setGates,
    setSearch,
    setAuto,
    reload,
  };
}
