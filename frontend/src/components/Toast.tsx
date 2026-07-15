/** 轻量 Toast：6s 后自动消失，可手动点 ×；按 tone 显示图标与配色。 */
import { useEffect, useRef } from "react";
import { CircleAlert, CircleCheck, Info, TriangleAlert, X } from "lucide-react";
import { useI18n } from "../i18n/LocaleContext";

export const TOAST_DURATION_MS = 6000;
export const TOAST_ERROR_DURATION_MS = 8000;

export type ToastTone = "info" | "success" | "error" | "warning";

type Props = {
  message: string;
  visible: boolean;
  durationMs?: number;
  sticky?: boolean;
  tone?: ToastTone;
  onDismiss: () => void;
};

function ToneIcon({ tone }: { tone: ToastTone }) {
  const props = { size: 16, strokeWidth: 2.25, "aria-hidden": true as const };
  switch (tone) {
    case "success":
      return <CircleCheck {...props} />;
    case "error":
      return <CircleAlert {...props} />;
    case "warning":
      return <TriangleAlert {...props} />;
    default:
      return <Info {...props} />;
  }
}

export function Toast({
  message,
  visible,
  durationMs = TOAST_DURATION_MS,
  sticky = false,
  tone = "info",
  onDismiss,
}: Props) {
  const { t } = useI18n();
  const onDismissRef = useRef(onDismiss);
  onDismissRef.current = onDismiss;

  useEffect(() => {
    if (!visible || !message || sticky) return;
    const id = setTimeout(() => onDismissRef.current(), durationMs);
    return () => clearTimeout(id);
  }, [visible, message, durationMs, sticky]);

  if (!visible || !message) return null;

  return (
    <div
      className={`astro-toast tone-${tone}${sticky ? " is-sticky" : ""}`}
      role={tone === "error" ? "alert" : "status"}
      aria-live={tone === "error" ? "assertive" : "polite"}
      data-tone={tone}
    >
      {sticky && <div className="astro-toast-sticky-mark" aria-hidden />}
      <div className="astro-toast-row">
        <span className={`astro-toast-icon tone-${tone}`} aria-hidden>
          <ToneIcon tone={tone} />
        </span>
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
