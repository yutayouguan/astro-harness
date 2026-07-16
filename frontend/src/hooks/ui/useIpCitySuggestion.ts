import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

type IpLocation = {
  city: string;
  region?: string | null;
  country?: string | null;
};

export type IpCitySuggestionState =
  | { status: "loading"; city: null }
  | { status: "success"; city: string }
  | { status: "failed"; city: null };

export function useIpCitySuggestion(enabled: boolean): IpCitySuggestionState {
  const [state, setState] = useState<IpCitySuggestionState>(
    enabled ? { status: "loading", city: null } : { status: "failed", city: null },
  );

  useEffect(() => {
    if (!enabled) {
      setState({ status: "failed", city: null });
      return;
    }

    let cancelled = false;
    setState({ status: "loading", city: null });

    void invoke<IpLocation>("infer_ip_location")
      .then((result) => {
        const city = result.city.trim();
        if (cancelled) return;
        if (!city) {
          setState({ status: "failed", city: null });
          return;
        }
        setState({ status: "success", city });
      })
      .catch(() => {
        if (!cancelled) setState({ status: "failed", city: null });
      });

    return () => {
      cancelled = true;
    };
  }, [enabled]);

  return state;
}
