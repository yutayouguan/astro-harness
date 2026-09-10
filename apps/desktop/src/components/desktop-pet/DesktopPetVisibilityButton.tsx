import { invoke, isTauri } from "@tauri-apps/api/core";
import { PawPrint } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useDesktopPetState } from "../../hooks/app/useDesktopPetState";
import { useI18n } from "../../i18n/LocaleContext";
import { withDeadline } from "../../lib/ui/onboarding";

/** Use actual native visibility: enabled pets can still be hidden for presentations. */
export default function DesktopPetVisibilityButton({
  onError,
}: {
  onError: (message: string) => void;
}) {
  const { locale } = useI18n();
  const pet = useDesktopPetState();
  const [visible, setVisible] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const mounted = useRef(false);
  const pending = useRef(false);
  const request = useRef(0);
  const native = isTauri();
  const zh = locale === "zh";
  const label = !native
    ? zh
      ? "桌宠仅桌面端可用"
      : "Desktop pet is available in the desktop app"
    : visible === null
      ? zh
        ? "重新读取桌宠状态"
        : "Refresh desktop pet status"
      : visible
        ? zh
          ? "隐藏桌宠"
          : "Hide desktop pet"
        : zh
          ? "显示桌宠"
          : "Show desktop pet";

  const refresh = useCallback(async () => {
    const ticket = ++request.current;
    try {
      const next = await withDeadline(
        invoke<boolean>("get_desktop_pet_visible"),
        5000,
      );
      if (typeof next !== "boolean") throw new Error("Invalid pet visibility");
      if (mounted.current && ticket === request.current) setVisible(next);
      return next;
    } catch (error) {
      if (mounted.current && ticket === request.current) setVisible(null);
      throw error;
    }
  }, []);

  useEffect(() => {
    mounted.current = true;
    const onFocus = () => {
      if (native) void refresh().catch(() => {});
    };
    window.addEventListener("focus", onFocus);
    return () => {
      mounted.current = false;
      request.current++;
      window.removeEventListener("focus", onFocus);
    };
  }, [native, refresh]);

  useEffect(() => {
    // The existing subscription receives tray, settings and automatic fullscreen changes,
    // including events whose persisted revision has not changed.
    if (native) void refresh().catch(() => {});
  }, [native, pet.state, refresh]);

  const toggle = async () => {
    if (!native || pending.current) return;
    pending.current = true;
    setBusy(true);
    try {
      const current = await refresh();
      if (!mounted.current) return;
      if (visible === null) return; // An unknown state is a retry, never a guessed toggle.
      await pet.mutate(
        current ? "set_desktop_pet_enabled" : "resume_desktop_pet",
        current ? { enabled: false } : {},
      );
      const actual = await refresh();
      if (actual === current)
        throw new Error("Pet window did not change visibility");
    } catch {
      // A write may have succeeded while the native window operation failed.
      await refresh().catch(() => {});
      if (mounted.current)
        onError(
          zh
            ? "桌宠显隐切换未完成，请重试或前往偏好设置检查桌宠。"
            : "Could not change pet visibility. Retry or check Desktop Pet in Preferences.",
        );
    } finally {
      pending.current = false;
      if (mounted.current) setBusy(false);
    }
  };

  return (
    <button
      type="button"
      className="sidebar-settings-btn sidebar-footer-icon"
      data-sidebar-action="pet"
      aria-label={label}
      title={label}
      aria-pressed={visible ?? undefined}
      aria-busy={busy || undefined}
      disabled={!native || pet.loading || busy}
      onClick={() => void toggle()}
    >
      <PawPrint size={17} strokeWidth={1.8} aria-hidden />
    </button>
  );
}
