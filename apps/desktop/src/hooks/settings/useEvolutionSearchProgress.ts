import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { SearchProgressEvent } from "../../types";

export type UseEvolutionSearchProgress = {
  events: SearchProgressEvent[];
  latest: SearchProgressEvent | null;
  reset(): void;
};

export function useEvolutionSearchProgress(
  active: boolean,
): UseEvolutionSearchProgress {
  const [events, setEvents] = useState<SearchProgressEvent[]>([]);
  const eventsRef = useRef<SearchProgressEvent[]>([]);

  useEffect(() => {
    if (!active) return;
    if (
      typeof window === "undefined" ||
      !("__TAURI_INTERNALS__" in window)
    )
      return;
    let unlisten: (() => void) | undefined;
    void listen<SearchProgressEvent>(
      "evolution-search-progress",
      (ev) => {
        eventsRef.current = [...eventsRef.current, ev.payload];
        setEvents(eventsRef.current);
      },
    )
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => {});
    return () => {
      unlisten?.();
    };
  }, [active]);

  const reset = useCallback(() => {
    eventsRef.current = [];
    setEvents([]);
  }, []);

  const latest = events.length > 0 ? events[events.length - 1] : null;

  return { events, latest, reset };
}
