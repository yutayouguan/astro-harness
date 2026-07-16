/** 右栏上下文用量区：指标、环形图、可展开分项。 */
import { useMemo, useState } from "react";
import { ChevronDown } from "lucide-react";
import { useI18n } from "../i18n/LocaleContext";
import type { MessageKey } from "../i18n/messages";
import {
  formatTokenCount,
  usagePercent,
  visibleSegments,
  SEGMENT_TONE,
  type ContextUsageSnapshot,
} from "../lib/chat/contextUsage";

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

const SEG_HINT: Record<string, MessageKey> = {
  system: "chat.contextExplorer.hint.system",
  tools: "chat.contextExplorer.hint.tools",
  mcp: "chat.contextExplorer.hint.mcp",
  memory: "chat.contextExplorer.hint.memory",
  skills: "chat.contextExplorer.hint.skills",
  recall: "chat.contextExplorer.hint.recall",
  subagent: "chat.contextExplorer.hint.subagent",
  conversation: "chat.contextExplorer.hint.conversation",
};

const DONUT_SIZE = 148;
const DONUT_STROKE = 14;
const DONUT_RADIUS = (DONUT_SIZE - DONUT_STROKE) / 2;
const DONUT_C = 2 * Math.PI * DONUT_RADIUS;

type Props = {
  snapshot: ContextUsageSnapshot | null;
  windowTokens: number;
  sessionLabel: string;
};

export default function ContextExplorer({
  snapshot,
  windowTokens,
  sessionLabel,
}: Props) {
  const { t } = useI18n();
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set());
  const [expandAll, setExpandAll] = useState(false);

  const win = windowTokens > 0 ? windowTokens : 128_000;
  const used = snapshot?.totalTokens ?? 0;
  const pct = usagePercent(used, win);
  const segs = useMemo(
    () => (snapshot ? visibleSegments(snapshot) : []),
    [snapshot],
  );

  const isOpen = (id: string) => expandAll || expanded.has(id);

  const toggleRow = (id: string) => {
    if (expandAll) {
      setExpandAll(false);
      setExpanded(new Set(segs.filter((s) => s.id !== id).map((s) => s.id)));
      return;
    }
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const toggleExpandAll = () => {
    if (expandAll) {
      setExpandAll(false);
      setExpanded(new Set());
    } else {
      setExpandAll(true);
      setExpanded(new Set());
    }
  };

  let dashOffset = 0;
  const arcs = segs.map((s) => {
    const len = Math.max(0, (s.tokens / win) * DONUT_C);
    const start = dashOffset;
    dashOffset += len;
    return { id: s.id, len, start };
  });
  const restLen = Math.max(0, DONUT_C - dashOffset);

  return (
    <section className="ctx-explorer" aria-label={t("chat.contextUsage")}>
      <div className="ctx-explorer-metrics">
        <div className="ctx-explorer-metric">
          <span className="ctx-explorer-metric-label">{t("chat.rightPanel.context")}</span>
          <span className="ctx-explorer-metric-value ctx-explorer-session" title={sessionLabel}>
            {sessionLabel}
          </span>
        </div>
        <div className="ctx-explorer-metric">
          <span className="ctx-explorer-metric-label">{t("chat.contextExplorer.contextSize")}</span>
          <span className="ctx-explorer-metric-value">{formatTokenCount(win)}</span>
        </div>
        <div className="ctx-explorer-metric">
          <span className="ctx-explorer-metric-label">{t("chat.contextExplorer.tokensUsed")}</span>
          <span className="ctx-explorer-metric-value">~{formatTokenCount(used)}</span>
        </div>
      </div>

      <p className="ctx-explorer-blurb">{t("chat.contextExplorer.blurb")}</p>

      <div className="ctx-donut-wrap">
        <svg
          className="ctx-donut"
          width={DONUT_SIZE}
          height={DONUT_SIZE}
          viewBox={`0 0 ${DONUT_SIZE} ${DONUT_SIZE}`}
          role="img"
          aria-label={t("chat.contextUsageFull", { pct: String(pct) })}
        >
          <g transform={`rotate(-90 ${DONUT_SIZE / 2} ${DONUT_SIZE / 2})`}>
            {arcs.map((arc) => {
              const toneVar = SEGMENT_TONE[arc.id] ?? "--accent";
              return (
                <circle
                  key={arc.id}
                  className="ctx-donut-arc"
                  cx={DONUT_SIZE / 2}
                  cy={DONUT_SIZE / 2}
                  r={DONUT_RADIUS}
                  fill="none"
                  stroke={`var(${toneVar})`}
                  strokeWidth={DONUT_STROKE}
                  strokeDasharray={`${arc.len} ${DONUT_C - arc.len}`}
                  strokeDashoffset={-arc.start}
                  strokeLinecap="butt"
                />
              );
            })}
            {restLen > 0 && (
              <circle
                className="ctx-donut-rest"
                cx={DONUT_SIZE / 2}
                cy={DONUT_SIZE / 2}
                r={DONUT_RADIUS}
                fill="none"
                stroke="var(--ink-mute)"
                strokeWidth={DONUT_STROKE}
                strokeDasharray={`${restLen} ${DONUT_C - restLen}`}
                strokeDashoffset={-dashOffset}
                strokeLinecap="butt"
                opacity={0.28}
              />
            )}
          </g>
        </svg>
        <div className="ctx-donut-center" aria-hidden>
          <span className="ctx-donut-pct">{pct}%</span>
        </div>
      </div>

      <div className="ctx-explorer-list-head">
        <h3 className="ctx-explorer-list-title">{t("chat.contextExplorer.breakdown")}</h3>
        {segs.length > 0 && (
          <button
            type="button"
            className="ctx-explorer-expand-all"
            onClick={toggleExpandAll}
          >
            {expandAll
              ? t("chat.contextExplorer.collapseAll")
              : t("chat.contextExplorer.expandAll")}
          </button>
        )}
      </div>

      {!snapshot || segs.length === 0 ? (
        <p className="ctx-explorer-empty">{t("chat.contextUsageEmpty")}</p>
      ) : (
        <ul className="ctx-explorer-list">
          {segs.map((s) => {
            const labelKey = SEG_LABEL[s.id];
            const label = labelKey ? t(labelKey) : s.id;
            const hintKey = SEG_HINT[s.id];
            const hint = hintKey ? t(hintKey) : null;
            const toneVar = SEGMENT_TONE[s.id] ?? "--accent";
            const open = isOpen(s.id);
            return (
              <li key={s.id} className={`ctx-explorer-row ${open ? "is-open" : ""}`}>
                <button
                  type="button"
                  className="ctx-explorer-row-btn"
                  onClick={() => toggleRow(s.id)}
                  aria-expanded={open}
                >
                  <ChevronDown
                    className="ctx-explorer-chevron"
                    size={14}
                    strokeWidth={2.2}
                    aria-hidden
                  />
                  <span
                    className="ctx-explorer-swatch"
                    style={{ background: `var(${toneVar})` }}
                    aria-hidden
                  />
                  <span className="ctx-explorer-row-label">
                    {label}
                    {s.count != null && s.count > 0 ? (
                      <span className="ctx-explorer-count"> ({s.count})</span>
                    ) : null}
                  </span>
                  <span className="ctx-explorer-row-tokens">
                    ~{formatTokenCount(s.tokens)}
                  </span>
                </button>
                {open && hint ? (
                  <p className="ctx-explorer-row-hint">{hint}</p>
                ) : null}
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
