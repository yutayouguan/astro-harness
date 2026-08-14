/** 整轮流式生成期间常驻的点阵加载指示，带退场动画。 */
import { useEffect, useRef, useState } from "react";
import { useI18n } from "../../i18n/LocaleContext";

const CELLS = 9;
const EXIT_MS = 140;

type Props = {
  /** 无正文时作为主占位，有正文时贴在底部 */
  alone?: boolean;
  /** 控制显隐（false 时播放退场后卸载） */
  visible?: boolean;
};

export default function MsgStreamLoader({ alone = false, visible = true }: Props) {
  const { t } = useI18n();
  const [mounted, setMounted] = useState(visible);
  const [leaving, setLeaving] = useState(false);
  const timerRef = useRef<ReturnType<typeof setTimeout>>();

  useEffect(() => {
    if (visible) {
      setMounted(true);
      setLeaving(false);
      clearTimeout(timerRef.current);
    } else if (mounted) {
      const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
      if (reduced) {
        setMounted(false);
      } else {
        setLeaving(true);
        timerRef.current = setTimeout(() => setMounted(false), EXIT_MS);
      }
    }
    return () => clearTimeout(timerRef.current);
  }, [visible]);

  if (!mounted) return null;

  return (
    <div
      className={`msg-stream-loader${alone ? " is-alone" : ""}${leaving ? " is-leaving" : ""}`}
      aria-label={t("chat.generating")}
      role="status"
    >
      {Array.from({ length: CELLS }, (_, i) => (
        <span
          key={i}
          style={{ animationDelay: `${(i % 3) * 0.12 + Math.floor(i / 3) * 0.08}s` }}
        />
      ))}
    </div>
  );
}
