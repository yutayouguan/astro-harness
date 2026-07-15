/** 轻量 Toast：顶部倒计时进度条，走完或点击关闭。 */
import { useEffect, useRef } from "react";
import { X } from "lucide-react";
import { useI18n } from "../i18n/LocaleContext";

const DEFAULT_DURATION_MS = 4000;

/** Toast 入参 */
type Props = {
  message: string;
  visible: boolean;
  /** 展示时长；进度条从满回退到空后回调 onDismiss */
  durationMs?: number;
  onDismiss: () => void;
};

export function Toast({
  message,
  visible,
  durationMs = DEFAULT_DURATION_MS,
  onDismiss,
}: Props) {
  const { t } = useI18n();
  const onDismissRef = useRef(onDismiss);
  onDismissRef.current = onDismiss;

  useEffect(() => {
    if (!visible || !message) return;
    const id = window.setTimeout(() => onDismissRef.current(), durationMs);
    return () => window.clearTimeout(id);
  }, [visible, message, durationMs]);

  if (!visible || !message) return null;

  return (
    <div className="astro-toast" role="status" aria-live="polite">
      <div className="astro-toast-progress" aria-hidden>
        <div
          key={`${message}:${durationMs}`}
          className="astro-toast-progress-bar"
          style={{ animationDuration: `${durationMs}ms` }}
        />
      </div>
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
