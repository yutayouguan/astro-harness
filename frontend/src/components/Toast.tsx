/** 轻量 Toast：边框倒计时动画驱动关闭，可手动点 ×；按 tone 显示图标与配色。 */
import { useEffect, useRef } from "react";
import { CircleAlert, CircleCheck, Info, TriangleAlert, X } from "lucide-react";
import { useI18n } from "../i18n/LocaleContext";

export const TOAST_DURATION_MS = 4000;
export const TOAST_ERROR_DURATION_MS = 6000;

export type ToastTone = "info" | "success" | "error" | "warning";

/** Toast 入参 */
type Props = {
  message: string;
  visible: boolean;
  /** 展示时长；进度走到 0 后 onDismiss */
  durationMs?: number;
  /** true：不自动消失，须点关闭（错误/阻断类） */
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
  const borderRef = useRef<SVGRectElement>(null);

  useEffect(() => {
    const el = borderRef.current;
    if (!visible || !message || sticky || !el) return;

    // 用 SVG 实际渲染尺寸设定 rect 大小，确保 getTotalLength() 精确
    const svgEl = el.ownerSVGElement!;
    const { width: svgW, height: svgH } = svgEl.getBoundingClientRect();
    el.setAttribute("width", String(Math.max(0, svgW - 2)));
    el.setAttribute("height", String(Math.max(0, svgH - 2)));

    const perimeter = el.getTotalLength();
    el.style.strokeDasharray = String(perimeter);
    el.style.strokeDashoffset = "0";

    const started = performance.now();
    let raf = 0;
    const tick = (now: number) => {
      const remaining = Math.max(0, 1 - (now - started) / durationMs);
      if (borderRef.current) {
        // 负 offset：缺口从路径起点（左上角）顺时针扩大，边框顺时针消退
        borderRef.current.style.strokeDashoffset = String(
          -perimeter * (1 - remaining),
        );
      }
      if (remaining <= 0) {
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
      {!sticky && (
        <svg
          className="astro-toast-border"
          width="100%"
          height="100%"
          aria-hidden
        >
          <rect
            ref={borderRef}
            className="astro-toast-border-rect"
            x={1}
            y={1}
            rx={13}
            ry={13}
          />
        </svg>
      )}
    </div>
  );
}
