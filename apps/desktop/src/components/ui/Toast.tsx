/** 轻量 Toast：倒计时环绕进度条后自动消失，可手动点 ×；按 tone 显示图标与配色。 */
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { CircleAlert, Info, TriangleAlert, X } from "lucide-react";
import { AnimatePresence, motion, useReducedMotion } from "framer-motion";
import { useI18n } from "../../i18n/LocaleContext";

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

/** success：圆圈描完再画勾（约 360ms）；其余 tone 仍用 Lucide 静态图标。 */
function AnimatedSuccessIcon() {
  return (
    <svg
      className="astro-toast-success-icon"
      width={16}
      height={16}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2.25}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <circle
        className="astro-toast-success-circle"
        cx={12}
        cy={12}
        r={10}
        pathLength={1}
      />
      <path
        className="astro-toast-success-check"
        d="M9 12l2 2 4-4"
        pathLength={1}
      />
    </svg>
  );
}

function ToneIcon({ tone }: { tone: ToastTone }) {
  const props = { size: 16, strokeWidth: 2.25, "aria-hidden": true as const };
  switch (tone) {
    case "success":
      return <AnimatedSuccessIcon />;
    case "error":
      return <CircleAlert {...props} />;
    case "warning":
      return <TriangleAlert {...props} />;
    default:
      return <Info {...props} />;
  }
}

/** 贴边环绕倒计时：仅进度描边、无轨道；pathLength=100，dashoffset 0→100。 */
const TOAST_RADIUS_PX = 14;
const RING_STROKE_PX = 2.5;
/** 外边到描边外沿的空隙；中心线再内收半线宽，避免 overflow 裁切不匀 */
const RING_GAP_PX = 2;

function ToastCountdownRing({ durationMs }: { durationMs: number }) {
  const svgRef = useRef<SVGSVGElement>(null);
  const [box, setBox] = useState({ w: 0, h: 0 });

  useLayoutEffect(() => {
    const host = svgRef.current?.parentElement;
    if (!host) return;
    const update = () => {
      const { width, height } = host.getBoundingClientRect();
      setBox({ w: width, h: height });
    };
    update();
    const ro = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (!entry) return;
      const boxSize = entry.borderBoxSize?.[0];
      if (boxSize) {
        setBox({ w: boxSize.inlineSize, h: boxSize.blockSize });
        return;
      }
      const { width, height } = host.getBoundingClientRect();
      setBox({ w: width, h: height });
    });
    ro.observe(host);
    return () => ro.disconnect();
  }, []);

  const inset = RING_GAP_PX + RING_STROKE_PX / 2;
  const rx = Math.max(0, TOAST_RADIUS_PX - inset);
  const ready = box.w > 4 && box.h > 4;

  return (
    <svg
      ref={svgRef}
      className="astro-toast-ring"
      width={ready ? box.w : 0}
      height={ready ? box.h : 0}
      viewBox={ready ? `0 0 ${box.w} ${box.h}` : undefined}
      aria-hidden
    >
      {ready && (
        <rect
          className="astro-toast-ring-progress"
          x={inset}
          y={inset}
          width={Math.max(0, box.w - inset * 2)}
          height={Math.max(0, box.h - inset * 2)}
          rx={rx}
          ry={rx}
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
  const reducedMotion = useReducedMotion();
  const onDismissRef = useRef(onDismiss);
  onDismissRef.current = onDismiss;

  useEffect(() => {
    if (!visible || !message || sticky) return;
    const id = setTimeout(() => onDismissRef.current(), durationMs);
    return () => clearTimeout(id);
  }, [visible, message, durationMs, sticky]);

  if (typeof document === "undefined") return null;

  // Portal 到 body：避免 chat/media 等祖先的 transform/filter
  // 把 position:fixed 变成相对该祖先定位（表现为「贴在内容区」而非视口右下角）。
  return createPortal(
    <AnimatePresence initial={false}>
      {visible && message ? (
        <motion.div
          key="toast"
          className={`astro-toast tone-${tone}${sticky ? " is-sticky" : ""}`}
          role={tone === "error" ? "alert" : "status"}
          aria-live={tone === "error" ? "assertive" : "polite"}
          data-tone={tone}
          initial={
            reducedMotion ? { opacity: 0 } : { opacity: 0, y: 8, scale: 0.97 }
          }
          animate={{ opacity: 1, y: 0, scale: 1 }}
          exit={
            reducedMotion
              ? { opacity: 0, transition: { duration: 0.12 } }
              : {
                  opacity: 0,
                  y: 6,
                  scale: 0.985,
                  transition: { duration: 0.14, ease: "easeOut" },
                }
          }
          transition={{
            duration: reducedMotion ? 0.12 : 0.22,
            ease: [0.22, 1, 0.36, 1],
          }}
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
        </motion.div>
      ) : null}
    </AnimatePresence>,
    document.body,
  );
}
