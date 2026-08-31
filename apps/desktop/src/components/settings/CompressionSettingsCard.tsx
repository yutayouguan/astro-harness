/** 偏好设置：上下文卫生（压缩阈值与保护参数）。 */
import { useCallback, useEffect, useState, type ReactNode } from "react";
import {
  ArrowDownToLine,
  ArrowUpToLine,
  Feather,
  Flame,
  FoldVertical,
  Gauge,
  Hash,
  Layers3,
  Lightbulb,
  MessagesSquare,
  RefreshCw,
  RotateCcw,
  Scale,
  Shield,
  ShieldCheck,
  ShieldPlus,
  SlidersHorizontal,
  Sparkles,
  TrendingDown,
  TriangleAlert,
  Wrench,
  type LucideIcon,
} from "lucide-react";
import { useCompressionSettings } from "../../hooks/settings/useCompressionSettings";
import { useI18n } from "../../i18n/LocaleContext";
import type { MessageKey } from "../../i18n/messages";
import type { CompressionSettingsDto } from "../../types";

type Props = {
  tone?: string;
  active?: boolean;
};

type StageId = "soft" | "medium" | "hard";

type NumField = {
  key: keyof CompressionSettingsDto;
  labelKey: MessageKey;
  hintKey?: MessageKey;
  /** 界面用百分比展示（存 0–1） */
  percent?: boolean;
  Icon?: LucideIcon;
};

function pctDraft(ratio: number): string {
  return String(Math.round(ratio * 1000) / 10);
}

function parsePct(raw: string): number | null {
  const n = Number(raw.trim());
  if (!Number.isFinite(n)) return null;
  return n / 100;
}

function parseIntDraft(raw: string): number | null {
  const n = Number(raw.trim());
  if (!Number.isFinite(n)) return null;
  return Math.round(n);
}

const STAGE_META: {
  id: StageId;
  ratioKey: "softRatio" | "mediumRatio" | "hardRatio";
  labelKey: MessageKey;
  hintKey: MessageKey;
  Icon: LucideIcon;
}[] = [
  {
    id: "soft",
    ratioKey: "softRatio",
    labelKey: "prefs.context.soft",
    hintKey: "prefs.context.softDesc",
    Icon: Feather,
  },
  {
    id: "medium",
    ratioKey: "mediumRatio",
    labelKey: "prefs.context.medium",
    hintKey: "prefs.context.mediumDesc",
    Icon: Scale,
  },
  {
    id: "hard",
    ratioKey: "hardRatio",
    labelKey: "prefs.context.hard",
    hintKey: "prefs.context.hardDesc",
    Icon: Flame,
  },
];

const BUDGET_META: {
  id: StageId;
  titleKey: MessageKey;
  Icon: LucideIcon;
  fields: NumField[];
}[] = [
  {
    id: "soft",
    titleKey: "prefs.context.budgetSoft",
    Icon: Feather,
    fields: [
      { key: "softMaxChars", labelKey: "prefs.context.maxChars", Icon: Hash },
      {
        key: "softHeadChars",
        labelKey: "prefs.context.headChars",
        Icon: ArrowUpToLine,
      },
      {
        key: "softTailChars",
        labelKey: "prefs.context.tailChars",
        Icon: ArrowDownToLine,
      },
    ],
  },
  {
    id: "medium",
    titleKey: "prefs.context.budgetMedium",
    Icon: Scale,
    fields: [
      { key: "mediumMaxChars", labelKey: "prefs.context.maxChars", Icon: Hash },
      {
        key: "mediumHeadChars",
        labelKey: "prefs.context.headChars",
        Icon: ArrowUpToLine,
      },
      {
        key: "mediumTailChars",
        labelKey: "prefs.context.tailChars",
        Icon: ArrowDownToLine,
      },
    ],
  },
  {
    id: "hard",
    titleKey: "prefs.context.budgetHard",
    Icon: Flame,
    fields: [
      { key: "hardMaxChars", labelKey: "prefs.context.maxChars", Icon: Hash },
      {
        key: "hardHeadChars",
        labelKey: "prefs.context.headChars",
        Icon: ArrowUpToLine,
      },
      {
        key: "hardTailChars",
        labelKey: "prefs.context.tailChars",
        Icon: ArrowDownToLine,
      },
    ],
  },
];

const OTHER_FIELDS: NumField[] = [
  {
    key: "toolResultsLimit",
    labelKey: "prefs.context.toolResultsLimit",
    hintKey: "prefs.context.toolResultsLimitDesc",
    Icon: Wrench,
  },
  {
    key: "midRunSummaryRatio",
    labelKey: "prefs.context.midRun",
    hintKey: "prefs.context.midRunDesc",
    percent: true,
    Icon: FoldVertical,
  },
  {
    key: "recommendCompactRatio",
    labelKey: "prefs.context.recommend",
    hintKey: "prefs.context.recommendDesc",
    percent: true,
    Icon: Lightbulb,
  },
  {
    key: "protectLastN",
    labelKey: "prefs.context.protectLast",
    hintKey: "prefs.context.protectLastDesc",
    Icon: ShieldCheck,
  },
  {
    key: "protectFirstMessages",
    labelKey: "prefs.context.protectFirst",
    hintKey: "prefs.context.protectFirstDesc",
    Icon: ShieldPlus,
  },
  {
    key: "thrashingMinGainRatio",
    labelKey: "prefs.context.thrashingGain",
    hintKey: "prefs.context.thrashingGainDesc",
    percent: true,
    Icon: TrendingDown,
  },
  {
    key: "thrashingMaxConsecutive",
    labelKey: "prefs.context.thrashingMax",
    hintKey: "prefs.context.thrashingMaxDesc",
    Icon: TriangleAlert,
  },
  {
    key: "keepTailBubbles",
    labelKey: "prefs.context.keepTail",
    hintKey: "prefs.context.keepTailDesc",
    Icon: MessagesSquare,
  },
];

function SectionHead({
  Icon,
  children,
}: {
  Icon: LucideIcon;
  children: ReactNode;
}) {
  return (
    <h3 className="prefs-toggle-heading prefs-context-section-head">
      <span className="prefs-context-section-ico" aria-hidden>
        <Icon size={13} strokeWidth={2.25} />
      </span>
      <span>{children}</span>
    </h3>
  );
}

export default function CompressionSettingsCard({
  tone = "pink",
  active = true,
}: Props) {
  const { t } = useI18n();
  const { loading, error, settings, save, reset, reload } =
    useCompressionSettings(active);
  const [draft, setDraft] = useState<CompressionSettingsDto | null>(null);
  const [localError, setLocalError] = useState<string | null>(null);

  useEffect(() => {
    if (settings) setDraft(settings);
  }, [settings]);

  const commit = useCallback(
    async (next: CompressionSettingsDto) => {
      setLocalError(null);
      if (!(
        next.softRatio < next.mediumRatio && next.mediumRatio < next.hardRatio
      )) {
        setLocalError(t("prefs.context.invalidOrder"));
        if (settings) setDraft(settings);
        return;
      }
      try {
        await save(next);
      } catch {
        if (settings) setDraft(settings);
      }
    },
    [save, settings, t],
  );

  const setField = <K extends keyof CompressionSettingsDto>(
    key: K,
    value: CompressionSettingsDto[K],
  ) => {
    setDraft((prev) => (prev ? { ...prev, [key]: value } : prev));
  };

  const commitRatio = (
    key:
      | "softRatio"
      | "mediumRatio"
      | "hardRatio"
      | "midRunSummaryRatio"
      | "recommendCompactRatio"
      | "thrashingMinGainRatio",
    raw: string,
  ) => {
    if (!draft) return;
    const parsed = parsePct(raw);
    if (parsed == null) {
      if (settings) setDraft(settings);
      return;
    }
    void commit({ ...draft, [key]: parsed });
  };

  const commitInt = (key: keyof CompressionSettingsDto, raw: string) => {
    if (!draft) return;
    const parsed = parseIntDraft(raw);
    if (parsed == null) {
      if (settings) setDraft(settings);
      return;
    }
    void commit({ ...draft, [key]: parsed });
  };

  const displayError = localError ?? error;

  return (
    <section className="prefs-card prefs-context-card">
      <div className="prefs-card-head">
        <div className="prefs-icon-badge" data-tone={tone} aria-hidden>
          <Layers3 width={22} height={22} strokeWidth={2} />
        </div>
        <div>
          <h2 className="prefs-card-title">{t("prefs.context.title")}</h2>
          <p className="prefs-card-sub">{t("prefs.context.sub")}</p>
        </div>
      </div>

      <p className="prefs-card-note">{t("prefs.context.viewHint")}</p>

      {displayError && (
        <p className="aux-error prefs-context-error" role="alert">
          {displayError}
        </p>
      )}

      {loading && !draft ? (
        <p className="prefs-card-note">{t("prefs.context.loading")}</p>
      ) : draft ? (
        <>
          <label className="prefs-toggle-row prefs-context-enable">
            <span className="prefs-toggle-icon" aria-hidden>
              <Sparkles size={15} strokeWidth={2.25} />
            </span>
            <span className="prefs-toggle-text">
              <span className="prefs-toggle-label">
                {t("prefs.context.enabled")}
              </span>
              <span className="prefs-toggle-desc">
                {t("prefs.context.enabledDesc")}
              </span>
            </span>
            <button
              type="button"
              role="switch"
              className="prefs-switch"
              aria-checked={draft.enabled}
              data-tone={tone}
              onClick={() => {
                const next = { ...draft, enabled: !draft.enabled };
                setDraft(next);
                void commit(next);
              }}
            >
              <span className="prefs-switch-thumb" />
            </button>
          </label>

          <SectionHead Icon={Gauge}>{t("prefs.context.stages")}</SectionHead>
          <div className="prefs-context-stages">
            {STAGE_META.map(({ id, ratioKey, labelKey, hintKey, Icon }) => (
              <label
                key={ratioKey}
                className="prefs-context-stage"
                data-stage={id}
              >
                <span className="prefs-context-stage-top">
                  <span className="prefs-context-stage-ico" aria-hidden>
                    <Icon size={15} strokeWidth={2.2} />
                  </span>
                  <span className="prefs-context-stage-text">
                    <span className="prefs-context-label">{t(labelKey)}</span>
                    <span className="prefs-context-hint">{t(hintKey)}</span>
                  </span>
                </span>
                <span className="prefs-context-input-wrap">
                  <input
                    className="aux-number-input"
                    type="number"
                    min={5}
                    max={95}
                    step={1}
                    value={pctDraft(draft[ratioKey])}
                    onChange={(e) => {
                      const p = parsePct(e.target.value);
                      if (p != null) setField(ratioKey, p);
                    }}
                    onBlur={(e) => commitRatio(ratioKey, e.target.value)}
                  />
                  <span className="prefs-context-suffix">%</span>
                </span>
              </label>
            ))}
          </div>

          <SectionHead Icon={SlidersHorizontal}>
            {t("prefs.context.budgets")}
          </SectionHead>
          <div className="prefs-context-budgets">
            {BUDGET_META.map((group) => {
              const StageIcon = group.Icon;
              return (
                <div
                  key={group.titleKey}
                  className="prefs-context-budget-block"
                  data-stage={group.id}
                >
                  <h4 className="prefs-context-budget-title">
                    <span className="prefs-context-budget-ico" aria-hidden>
                      <StageIcon size={13} strokeWidth={2.25} />
                    </span>
                    {t(group.titleKey)}
                  </h4>
                  <div className="prefs-context-grid prefs-context-grid--3">
                    {group.fields.map(({ key, labelKey, Icon }) => (
                      <label key={key} className="prefs-context-field">
                        <span className="prefs-context-label-row">
                          {Icon && (
                            <Icon
                              size={12}
                              strokeWidth={2.25}
                              className="prefs-context-field-ico"
                              aria-hidden
                            />
                          )}
                          <span className="prefs-context-label">
                            {t(labelKey)}
                          </span>
                        </span>
                        <input
                          className="aux-number-input"
                          type="number"
                          min={0}
                          step={50}
                          value={draft[key] as number}
                          onChange={(e) => {
                            const n = parseIntDraft(e.target.value);
                            if (n != null) setField(key, n as never);
                          }}
                          onBlur={(e) => commitInt(key, e.target.value)}
                        />
                      </label>
                    ))}
                  </div>
                </div>
              );
            })}
          </div>

          <SectionHead Icon={Shield}>{t("prefs.context.triggers")}</SectionHead>
          <div className="prefs-context-grid">
            {OTHER_FIELDS.map(({ key, labelKey, hintKey, percent, Icon }) => (
              <label
                key={key}
                className="prefs-context-field prefs-context-field--card"
              >
                <span className="prefs-context-label-row">
                  {Icon && (
                    <span className="prefs-context-field-badge" aria-hidden>
                      <Icon size={12} strokeWidth={2.25} />
                    </span>
                  )}
                  <span className="prefs-context-label">{t(labelKey)}</span>
                </span>
                {hintKey && (
                  <span className="prefs-context-hint">{t(hintKey)}</span>
                )}
                <div className="prefs-context-input-wrap">
                  <input
                    className="aux-number-input"
                    type="number"
                    min={0}
                    step={1}
                    value={
                      percent
                        ? pctDraft(draft[key] as number)
                        : (draft[key] as number)
                    }
                    onChange={(e) => {
                      if (percent) {
                        const p = parsePct(e.target.value);
                        if (p != null) setField(key, p as never);
                      } else {
                        const n = parseIntDraft(e.target.value);
                        if (n != null) setField(key, n as never);
                      }
                    }}
                    onBlur={(e) => {
                      if (percent) {
                        commitRatio(
                          key as "midRunSummaryRatio",
                          e.target.value,
                        );
                      } else {
                        commitInt(key, e.target.value);
                      }
                    }}
                  />
                  {percent && <span className="prefs-context-suffix">%</span>}
                </div>
              </label>
            ))}
          </div>

          <div className="prefs-context-actions">
            <button
              type="button"
              className="prefs-diag-btn"
              data-tone={tone}
              onClick={() => void reload()}
              disabled={loading}
            >
              <RefreshCw size={14} strokeWidth={2.25} aria-hidden />
              {t("prefs.context.refresh")}
            </button>
            <button
              type="button"
              className="prefs-diag-btn"
              data-tone={tone}
              onClick={() => void reset()}
              disabled={loading}
            >
              <RotateCcw size={14} strokeWidth={2.25} aria-hidden />
              {t("prefs.context.reset")}
            </button>
          </div>
        </>
      ) : null}
    </section>
  );
}
