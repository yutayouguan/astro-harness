/** 思考过程折叠块。 */
import { useEffect, useRef, useState } from "react";
import { Lightbulb } from "lucide-react";
import {
  ChevronDown as ChevronDownData,
  ChevronUp as ChevronUpData,
} from "lucide";
import { useI18n } from "../../i18n/LocaleContext";
import { useLiveElapsedSec } from "../../hooks/chat/useLiveElapsedSec";
import { formatElapsedSec } from "../../lib/chat/elapsedSec";
import { MorphToggleIcon } from "../icons/MorphIcon";

/** 思考过程折叠块入参 */
type Props = {
  reasoning: string;
  /** 流式且尚未产出正文时视为「思考中」 */
  active: boolean;
  /** 整轮结束时最后一段思考的结果。 */
  outcome?: "done" | "error" | "interrupted";
  /** 思考耗时（秒），有则展示「用时 Xs」 */
  durationSec?: number;
  /** 思考起点（ms）；缺省时在 active 瞬间本地闩锁 */
  startedAtMs?: number;
  /** 非活动思考块的默认展开状态 */
  defaultOpen?: boolean;
  /** 右键菜单“展开/折叠全部”的单次消息覆盖 */
  forcedOpen?: boolean;
};

export default function MsgReasoning({
  reasoning,
  active,
  outcome = "done",
  durationSec,
  startedAtMs,
  defaultOpen = false,
  forcedOpen,
}: Props) {
  const { t } = useI18n();
  const [open, setOpen] = useState(active || defaultOpen);
  const [localStart, setLocalStart] = useState<number | null>(null);
  const wasActiveRef = useRef(false);

  useEffect(() => {
    if (forcedOpen != null) {
      setOpen(forcedOpen);
      if (!active) {
        wasActiveRef.current = false;
        setLocalStart(null);
      }
      return;
    }
    if (active) {
      if (!wasActiveRef.current) {
        setLocalStart(startedAtMs ?? Date.now());
      }
      wasActiveRef.current = true;
      setOpen(true);
    } else {
      wasActiveRef.current = false;
      setLocalStart(null);
      setOpen(defaultOpen);
    }
  }, [active, defaultOpen, forcedOpen, startedAtMs]);

  const liveStart = active ? (startedAtMs ?? localStart) : null;
  const liveSec = useLiveElapsedSec(active, liveStart);

  const label = active
    ? t("chat.thinking")
    : outcome === "error"
      ? t("chat.thinkingFailed")
      : outcome === "interrupted"
        ? t("chat.thinkingInterrupted")
        : t("chat.thinkingDone");
  const elapsedSec = active ? liveSec : durationSec;

  return (
    <div
      className={`msg-reasoning ${active ? "is-active" : ""} ${
        outcome === "error" ? "is-error" : ""
      } ${outcome === "interrupted" ? "is-interrupted" : ""} ${
        open ? "is-open" : ""
      }`}
    >
      <button
        type="button"
        className="msg-reasoning-toggle"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
      >
        <Lightbulb
          size={15}
          strokeWidth={1.75}
          className="msg-reasoning-icon"
          aria-hidden
        />
        <span className="msg-reasoning-label">{label}</span>
        {elapsedSec != null ? (
          <span
            className="msg-reasoning-duration"
            aria-label={formatElapsedSec(elapsedSec)}
          >
            <span aria-hidden>·</span>
            {formatElapsedSec(elapsedSec)}
          </span>
        ) : null}
        <MorphToggleIcon
          active={open}
          activeIcon={ChevronUpData}
          inactiveIcon={ChevronDownData}
          size={14}
          strokeWidth={1.75}
          className="msg-reasoning-chevron"
          aria-hidden
        />
      </button>
      {open ? <pre className="msg-reasoning-body">{reasoning}</pre> : null}
    </div>
  );
}
