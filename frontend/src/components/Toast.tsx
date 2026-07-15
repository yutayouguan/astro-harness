/** 轻量 Toast：单一倒计时驱动进度条与关闭，可手动点 ×。 */
import { useEffect, useRef } from "react";
import { X } from "lucide-react";
import { useI18n } from "../i18n/LocaleContext";

export const TOAST_DURATION_MS = 4000;
export const TOAST_ERROR_DURATION_MS = 6000;

/** Toast 入参 */
type Props = {
  message: string;
  visible: boolean;
  /** 展示时长；进度走到 0 后 onDismiss */
  durationMs?: number;
  /** true：不自动消失，须点关闭（错误/阻断类） */
  sticky?: boolean;
  onDismiss: () => void;
};

export function Toast({
  message,
  visible,
  durationMs = TOAST_DURATION_MS,
  sticky = false,
  onDismiss,
}: Props) {
  const { t } = useI18n();
  const onDismissRef = useRef(onDismiss);
  onDismissRef.current = onDismiss;
  const barRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!visible || !message || sticky) {
      if (barRef.current) barRef.current.style.transform = "scaleX(1)";
      return;
    }

    const bar = barRef.current;
    if (bar) bar.style.transform = "scaleX(1)";

    const started = performance.now();
    let raf = 0;
    const tick = (now: number) => {
      const left = Math.max(0, 1 - (now - started) / durationMs);
      if (barRef.current) {
        barRef.current.style.transform = `scaleX(${left})`;
      }
      if (left <= 0) {
        onDismissRef.current();
        return;
      }
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [visible, message, durationMs, sticky]);

  if (!visible || !message) return null;

  return (
    <div
      className={`astro-toast${sticky ? " is-sticky" : ""}`}
      role="status"
      aria-live="polite"
    >
      {!sticky ? (
        <div className="astro-toast-progress" aria-hidden>
          <div ref={barRef} className="astro-toast-progress-bar" />
        </div>
      ) : (
        <div className="astro-toast-sticky-mark" aria-hidden />
      )}
      <div className="astro-toast-row">
        <p className="astro-toast-msg">{message}</p>
        <button
          type="button"
          className="astro-toast-close"
          aria-label={t("common.close")}
          onClick={onDismiss}
        >
          <X size={14} strokeWidth={2.25} aria-hidden />
        </button>
      </div>
    </div>
  );
}
