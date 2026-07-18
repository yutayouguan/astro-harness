import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { EvalExampleDto } from "../../types";

type AddArgs = {
  skillId: string | null;
  task: string;
  expectations: string[];
  verdict: "pass" | "fail";
  sourceSession?: string | null;
};

type UseEvalExamples = {
  loading: boolean;
  error: string | null;
  examples: EvalExampleDto[];
  add(args: AddArgs): Promise<void>;
  remove(id: string): Promise<void>;
  reload(): Promise<void>;
};

function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

export function useEvalExamples(active = true): UseEvalExamples {
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [examples, setExamples] = useState<EvalExampleDto[]>([]);

  const reload = useCallback(async () => {
    if (!active) return;
    setLoading(true);
    setError(null);
    try {
      const next = await invoke<EvalExampleDto[]>("list_eval_examples");
      setExamples(next);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setLoading(false);
    }
  }, [active]);

  useEffect(() => {
    void reload();
  }, [reload]);

  const add = useCallback(async (args: AddArgs) => {
    setError(null);
    try {
      const next = await invoke<EvalExampleDto[]>("add_eval_example", {
        skillId: args.skillId,
        task: args.task,
        expectations: args.expectations,
        verdict: args.verdict,
        sourceSession: args.sourceSession ?? null,
      });
      setExamples(next);
    } catch (err) {
      setError(errorMessage(err));
    }
  }, []);

  const remove = useCallback(async (id: string) => {
    setError(null);
    try {
      const next = await invoke<EvalExampleDto[]>("remove_eval_example", { id });
      setExamples(next);
    } catch (err) {
      setError(errorMessage(err));
    }
  }, []);

  return { loading, error, examples, add, remove, reload };
}
