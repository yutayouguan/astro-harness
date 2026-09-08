import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  acceptDesktopPetState,
  createPetMutationQueue,
  EMPTY_DESKTOP_PET_STATE,
  subscribeDesktopPetState,
  type DesktopPetState,
} from "../../lib/ui/desktopPetState";

export function useDesktopPetState(active = true) {
  const [state, setState] = useState(EMPTY_DESKTOP_PET_STATE);
  const latest = useRef(state);
  const mounted = useRef(false);
  const queue = useRef(createPetMutationQueue());
  const [loading, setLoading] = useState(true);
  const [pending, setPending] = useState(0);
  const [error, setError] = useState("");
  const accept = useCallback((next: DesktopPetState) => {
    if (!mounted.current) return;
    const accepted = acceptDesktopPetState(latest.current, next);
    if (accepted !== latest.current) setError("");
    latest.current = accepted;
    setState(latest.current);
  }, []);
  const report = useCallback((cause: unknown) => {
    if (mounted.current)
      setError(cause instanceof Error ? cause.message : String(cause));
  }, []);

  useEffect(() => {
    mounted.current = true;
    if (!active || !("__TAURI_INTERNALS__" in window)) {
      setLoading(false);
      return () => {
        mounted.current = false;
      };
    }
    setLoading(true);
    const dispose = subscribeDesktopPetState({
      listen: (receive) =>
        listen<DesktopPetState>("desktop-pet-changed", ({ payload }) =>
          receive(payload),
        ),
      snapshot: () => invoke<DesktopPetState>("get_desktop_pet_state"),
      receive: accept,
      error: report,
      ready: () => setLoading(false),
    });
    return () => {
      mounted.current = false;
      dispose();
    };
  }, [accept, active, report]);

  const mutate = useCallback(
    async (command: string, args: Record<string, unknown>) => {
      setPending((count) => count + 1);
      setError("");
      try {
        return await queue.current(async () => {
          const next = await invoke<DesktopPetState>(command, args);
          accept(next);
          return next;
        });
      } catch (cause) {
        report(cause);
        throw cause;
      } finally {
        if (mounted.current) setPending((count) => count - 1);
      }
    },
    [accept, report],
  );
  return {
    state,
    loading,
    pending,
    error,
    mutate,
    clearError: () => setError(""),
  };
}
