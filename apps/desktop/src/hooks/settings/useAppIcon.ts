import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AppIconId, AppIconSettingsDto } from "../../types";
import { appIconSettingsWithFallback } from "../../lib/ui/appIconOptions";

type UseAppIcon = {
  loading: boolean;
  error: string | null;
  settings: AppIconSettingsDto;
  setIcon(variant: AppIconId): Promise<void>;
  reload(): Promise<void>;
};

function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

export function useAppIcon(active = true): UseAppIcon {
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [settings, setSettings] = useState<AppIconSettingsDto>(() =>
    appIconSettingsWithFallback(null),
  );

  const reload = useCallback(async () => {
    if (!active) return;
    setLoading(true);
    setError(null);
    try {
      const next = await invoke<AppIconSettingsDto>("get_app_icon");
      setSettings(appIconSettingsWithFallback(next));
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setLoading(false);
    }
  }, [active]);

  useEffect(() => {
    void reload();
  }, [reload]);

  const setIcon = useCallback(
    async (variant: AppIconId) => {
      setError(null);
      // 乐观更新当前选中，失败回滚
      setSettings((prev) => ({ ...prev, current: variant }));
      try {
        const next = await invoke<AppIconSettingsDto>("set_app_icon", {
          variant,
        });
        setSettings(appIconSettingsWithFallback(next, variant));
      } catch (err) {
        setError(errorMessage(err));
        void reload();
      }
    },
    [reload],
  );

  return { loading, error, settings, setIcon, reload };
}
