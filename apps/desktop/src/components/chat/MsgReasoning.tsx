/** 思考过程折叠块。 */
import { useEffect, useRef, useState } from "react";
import { Lightbulb } from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";
import { useLiveElapsedSec } from "../../hooks/chat/useLiveElapsedSec";
import { formatElapsedSec } from "../../lib/chat/elapsedSec";

/** 思考过程折叠块入参 */
type Props = {
  reasoning: string;
  /** 流式且尚未产出正文时视为「思考中」 */
  active: boolean;
  /** 思考耗时（秒），有则展示「用时 Xs」 */
  durationSec?: number;
  /** 思考起点（ms）；缺省时在 active 瞬间本地闩锁 */
  startedAtMs?: number;
};

export default function MsgReasoning({
  reasoning,
  active,
  durationSec,
  startedAtMs,
}: Props) {
  const { t } = useI18n();
  const [open, setOpen] = useState(active);
  const [localStart, setLocalStart] = useState<number | null>(null);
  const wasActiveRef = useRef(false);

  useEffect(() => {
    if (active) {
      if (!wasActiveRef.current) {
        setLocalStart(startedAtMs ?? Date.now());
      }
      wasActiveRef.current = true;
      setOpen(true);
    } else {
      wasActiveRef.current = false;
      setLocalStart(null);
      setOpen(false);
    }
  }, [active, startedAtMs]);

  const liveStart = active ? (startedAtMs ?? localStart) : null;
  const liveSec = useLiveElapsedSec(active, liveStart);

  const label =
    active && durationSec == null
      ? liveSec != null
        ? t("chat.thinkingWithTime", { s: formatElapsedSec(liveSec) })
        : t("chat.thinking")
      : durationSec != null
        ? t("chat.thinkingDoneWithTime", { s: formatElapsedSec(durationSec) })
        : t("chat.thinkingDone");

  return (
    <div className={`msg-reasoning ${active ? "is-active" : ""} ${open ? "is-open" : ""}`}>
      <button
        type="button"
        className="msg-reasoning-toggle"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
      >
        <Lightbulb size={15} strokeWidth={1.75} className="msg-reasoning-icon" aria-hidden />
        <span className="msg-reasoning-label">{label}</span>
      </button>
      {open ? <pre className="msg-reasoning-body">{reasoning}</pre> : null}
    </div>
  );
}
