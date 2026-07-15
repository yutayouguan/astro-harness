/** 轻量 Toast：倒计时环绕进度条后自动消失，可手动点 ×；按 tone 显示图标与配色。 */
import { useEffect, useLayoutEffect, useRef, useState } from "react";
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

/** 贴边环绕倒计时：仅进度描边、无轨道；pathLength=100，dashoffset 0→100。 */
function ToastCountdownRing({ durationMs }: { durationMs: number }) {
  const svgRef = useRef<SVGSVGElement>(null);
  const [box, setBox] = useState({ w: 0, h: 0 });

  useLayoutEffect(() => {
    const host = svgRef.current?.parentElement;
    if (!host) return;
    const update = () => {
      const { width, height } = host.getBoundingClientRect();
      setBox({ w: Math.round(width), h: Math.round(height) });
    };
    update();
    const ro = new ResizeObserver(update);
    ro.observe(host);
    return () => ro.disconnect();
  }, []);

  const inset = 2.5;
  const ready = box.w > 4 && box.h > 4;

  return (
    <svg
      ref={svgRef}
      className="astro-toast-ring"
      width={ready ? box.w : undefined}
      height={ready ? box.h : undefined}
      aria-hidden
    >
      {ready && (
        <rect
          className="astro-toast-ring-progress"
          x={inset}
          y={inset}
          width={box.w - inset * 2}
          height={box.h - inset * 2}
          rx={12}
          ry={12}
          pathLength={100}
          style={{ animationDuration: `${durationMs}ms` }}
        />
      )}
    </svg>
  );
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
      {!sticky && <ToastCountdownRing durationMs={durationMs} />}
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
