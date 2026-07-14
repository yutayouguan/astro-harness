/** Composer 上下文占用浮层：分段条、图例与「查看详情」。 */
import { useEffect, useRef, type RefObject } from "react";
import { X } from "lucide-react";
import { useI18n } from "../i18n/LocaleContext";
import type { MessageKey } from "../i18n/messages";
import {
  formatTokenCount,
  usagePercent,
  visibleSegments,
  SEGMENT_TONE,
  type ContextUsageSnapshot,
} from "../lib/contextUsage";
import ContextUsageBar from "./ContextUsageBar";

const SEG_LABEL: Record<string, MessageKey> = {
  system: "chat.contextSeg.system",
  tools: "chat.contextSeg.tools",
  mcp: "chat.contextSeg.mcp",
  memory: "chat.contextSeg.memory",
  skills: "chat.contextSeg.skills",
  recall: "chat.contextSeg.recall",
  subagent: "chat.contextSeg.subagent",
  conversation: "chat.contextSeg.conversation",
};

type Props = {
  open: boolean;
  snapshot: ContextUsageSnapshot | null;
  windowTokens: number;
  onClose: () => void;
  onViewDetails: () => void;
  /** 含触发按钮的外层；用于 click-outside，避免点按钮时先关再开 */
  containRef?: RefObject<HTMLElement | null>;
};

export default function ContextUsagePopover({
  open,
  snapshot,
  windowTokens,
  onClose,
  onViewDetails,
  containRef,
}: Props) {
  const { t } = useI18n();
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    const onPointer = (e: MouseEvent) => {
      const boundary = containRef?.current ?? rootRef.current;
      if (!boundary?.contains(e.target as Node)) onClose();
    };
    document.addEventListener("keydown", onKey);
    document.addEventListener("mousedown", onPointer);
    return () => {
      document.removeEventListener("keydown", onKey);
      document.removeEventListener("mousedown", onPointer);
    };
  }, [open, onClose, containRef]);

  if (!open) return null;

  const win = windowTokens > 0 ? windowTokens : 128_000;
  const used = snapshot?.totalTokens ?? 0;
  const pct = usagePercent(used, win);
  const segs = snapshot ? visibleSegments(snapshot) : [];

  return (
    <div
      className="ctx-usage-popover"
      ref={rootRef}
      role="dialog"
      aria-label={t("chat.contextUsage")}
    >
      <div className="ctx-usage-popover-header">
        <span className="ctx-usage-popover-title">{t("chat.contextUsage")}</span>
        <div className="ctx-usage-popover-header-actions">
          <button
            type="button"
            className="ctx-usage-popover-detail"
            onClick={() => {
              onViewDetails();
              onClose();
            }}
          >
            {t("chat.contextUsageDetail")}
          </button>
          <button
            type="button"
            className="ctx-usage-popover-close"
            onClick={onClose}
            aria-label={t("modelEdit.close")}
            title={t("modelEdit.close")}
          >
            <X size={14} strokeWidth={2.2} aria-hidden />
          </button>
        </div>
      </div>

      {snapshot ? (
        <div className="ctx-usage-popover-body">
          <p className="ctx-usage-popover-full">
            {t("chat.contextUsageFull", { pct: String(pct) })}
          </p>
          <p className="ctx-usage-popover-tokens">
            ~{formatTokenCount(used)} / {formatTokenCount(win)}
          </p>
          <ContextUsageBar snapshot={snapshot} windowTokens={win} />
          <ul className="ctx-usage-legend">
            {segs.map((s) => {
              const labelKey = SEG_LABEL[s.id];
              const label = labelKey ? t(labelKey) : s.id;
              const toneVar = SEGMENT_TONE[s.id] ?? "--accent";
              return (
                <li key={s.id} className="ctx-usage-legend-row">
                  <span
                    className="ctx-usage-legend-swatch"
                    style={{ background: `var(${toneVar})` }}
                    aria-hidden
                  />
                  <span className="ctx-usage-legend-label">{label}</span>
                  <span className="ctx-usage-legend-tokens">
                    ~{formatTokenCount(s.tokens)}
                  </span>
                </li>
              );
            })}
          </ul>
        </div>
      ) : (
        <p className="ctx-usage-popover-empty">{t("chat.contextUsageEmpty")}</p>
      )}
    </div>
  );
}
