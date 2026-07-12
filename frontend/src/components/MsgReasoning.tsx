/** 思考过程折叠块。 */
import { useEffect, useState } from "react";
import { Lightbulb } from "lucide-react";
import { useI18n } from "../i18n/LocaleContext";

/** 思考过程折叠块入参 */
type Props = {
  reasoning: string;
  /** 流式且尚未产出正文时视为「思考中」 */
  active: boolean;
  /** 思考耗时（秒），有则展示「用时 Xs」 */
  durationSec?: number;
};

function formatDuration(sec: number): string {
  if (sec < 10) return sec.toFixed(1);
  return String(Math.round(sec));
}

export default function MsgReasoning({
  reasoning,
  active,
  durationSec,
}: Props) {
  const { t } = useI18n();
  const [open, setOpen] = useState(active);

  useEffect(() => {
    if (active) setOpen(true);
    else setOpen(false);
  }, [active]);

  const label = active
    ? t("chat.thinking")
    : durationSec != null && durationSec > 0
      ? t("chat.thinkingDoneWithTime", { s: formatDuration(durationSec) })
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
