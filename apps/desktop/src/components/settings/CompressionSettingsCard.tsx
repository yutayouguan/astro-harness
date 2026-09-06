/** 偏好设置：上下文卫生（压缩阈值与保护参数）。 */
import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
} from "react";
import {
  ArrowDownToLine,
  ArrowUpToLine,
  ChevronDown,
  Feather,
  Flame,
  FoldVertical,
  Gauge,
  Hash,
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

type RangeField = NumField & {
  min: number;
  max: number;
  step?: number;
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
}[] = [
  {
    id: "soft",
    ratioKey: "softRatio",
    labelKey: "prefs.context.soft",
    hintKey: "prefs.context.softDesc",
  },
  {
    id: "medium",
    ratioKey: "mediumRatio",
    labelKey: "prefs.context.medium",
    hintKey: "prefs.context.mediumDesc",
  },
  {
    id: "hard",
    ratioKey: "hardRatio",
    labelKey: "prefs.context.hard",
    hintKey: "prefs.context.hardDesc",
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

const OTHER_FIELDS: RangeField[] = [
  {
    key: "toolResultsLimit",
    labelKey: "prefs.context.toolResultsLimit",
    hintKey: "prefs.context.toolResultsLimitDesc",
    Icon: Wrench,
    min: 0,
    max: 200,
  },
  {
    key: "midRunSummaryRatio",
    labelKey: "prefs.context.midRun",
    hintKey: "prefs.context.midRunDesc",
    percent: true,
    Icon: FoldVertical,
    min: 5,
    max: 99,
  },
  {
    key: "recommendCompactRatio",
    labelKey: "prefs.context.recommend",
    hintKey: "prefs.context.recommendDesc",
    percent: true,
    Icon: Lightbulb,
    min: 5,
    max: 99,
  },
  {
    key: "protectLastN",
    labelKey: "prefs.context.protectLast",
    hintKey: "prefs.context.protectLastDesc",
    Icon: ShieldCheck,
    min: 1,
    max: 200,
  },
  {
    key: "protectFirstMessages",
    labelKey: "prefs.context.protectFirst",
    hintKey: "prefs.context.protectFirstDesc",
    Icon: ShieldPlus,
    min: 1,
    max: 50,
  },
  {
    key: "thrashingMinGainRatio",
    labelKey: "prefs.context.thrashingGain",
    hintKey: "prefs.context.thrashingGainDesc",
    percent: true,
    Icon: TrendingDown,
    min: 1,
    max: 50,
  },
  {
    key: "thrashingMaxConsecutive",
    labelKey: "prefs.context.thrashingMax",
    hintKey: "prefs.context.thrashingMaxDesc",
    Icon: TriangleAlert,
    min: 1,
    max: 20,
  },
  {
    key: "keepTailBubbles",
    labelKey: "prefs.context.keepTail",
    hintKey: "prefs.context.keepTailDesc",
    Icon: MessagesSquare,
    min: 1,
    max: 50,
  },
];

function CompressionRange({
  label,
  value,
  min,
  max,
  step = 1,
  suffix,
  onChange,
  onCommit,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  step?: number;
  suffix?: string;
  onChange(value: number): void;
  onCommit(value: number): void;
}) {
  const lastCommittedValue = useRef(value);
  const changing = useRef(false);
  const safeValue = Math.min(max, Math.max(min, value));
  const progress = max === min ? 0 : ((safeValue - min) / (max - min)) * 100;
  const displayValue = `${safeValue}${suffix ?? ""}`;
  const style = {
    "--range-progress": `${progress}%`,
  } as CSSProperties;
  const changeValue = (nextValue: number) => {
    changing.current = true;
    onChange(nextValue);
  };
  const commitValue = (nextValue: number) => {
    changing.current = false;
    if (lastCommittedValue.current === nextValue) return;
    lastCommittedValue.current = nextValue;
    onCommit(nextValue);
  };

  useEffect(() => {
    if (!changing.current) lastCommittedValue.current = safeValue;
  }, [safeValue]);

  return (
    <span className="prefs-context-range" style={style}>
      <input
        className="prefs-context-range-input"
        type="range"
        aria-label={label}
        aria-valuetext={displayValue}
        min={min}
        max={max}
        step={step}
        value={safeValue}
        onChange={(event) => changeValue(Number(event.target.value))}
        onPointerUp={(event) => commitValue(Number(event.currentTarget.value))}
        onPointerCancel={(event) =>
          commitValue(Number(event.currentTarget.value))
        }
        onKeyUp={(event) => {
          if (
            [
              "ArrowLeft",
              "ArrowRight",
              "ArrowUp",
              "ArrowDown",
              "Home",
              "End",
            ].includes(event.key)
          ) {
            commitValue(Number(event.currentTarget.value));
          }
        }}
        onBlur={(event) => commitValue(Number(event.currentTarget.value))}
      />
      <output className="prefs-context-range-value" aria-hidden="true">
        {displayValue}
      </output>
    </span>
  );
}

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
      if (
        !(
          next.softRatio < next.mediumRatio && next.mediumRatio < next.hardRatio
        )
      ) {
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
      <header className="prefs-context-page-head">
        <span className="prefs-context-eyebrow" aria-hidden="true">
          Context
        </span>
        <h2>{t("prefs.context.title")}</h2>
        <p>{t("prefs.context.sub")}</p>
      </header>

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
            {STAGE_META.map(({ id, ratioKey, labelKey, hintKey }, index) => {
              const value = Number(pctDraft(draft[ratioKey]));
              const previous =
                index === 0
                  ? 5
                  : Math.ceil(draft[STAGE_META[index - 1].ratioKey] * 100) + 1;
              const next =
                index === STAGE_META.length - 1
                  ? 95
                  : Math.floor(draft[STAGE_META[index + 1].ratioKey] * 100) - 1;
              return (
                <label
                  key={ratioKey}
                  className="prefs-context-stage"
                  data-stage={id}
                >
                  <span className="prefs-context-stage-accent" aria-hidden />
                  <span className="prefs-context-stage-top">
                    <span className="prefs-context-stage-text">
                      <span className="prefs-context-label">{t(labelKey)}</span>
                      <span className="prefs-context-hint">{t(hintKey)}</span>
                    </span>
                  </span>
                  <CompressionRange
                    label={t(labelKey)}
                    value={value}
                    min={previous}
                    max={next}
                    suffix="%"
                    onChange={(nextValue) =>
                      setField(ratioKey, nextValue / 100)
                    }
                    onCommit={(nextValue) =>
                      commitRatio(ratioKey, String(nextValue))
                    }
                  />
                </label>
              );
            })}
          </div>

          <details className="prefs-context-advanced">
            <summary>
              <span>
                <SlidersHorizontal size={14} strokeWidth={2.25} aria-hidden />
                {t("prefs.context.budgets")}
              </span>
              <span className="prefs-context-advanced-meta">
                <small>{t("prefs.context.triggers")}</small>
                <ChevronDown size={14} strokeWidth={2.25} aria-hidden />
              </span>
            </summary>

            <div className="prefs-context-advanced-body">
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

              <SectionHead Icon={Shield}>
                {t("prefs.context.triggers")}
              </SectionHead>
              <div className="prefs-context-controls">
                {OTHER_FIELDS.map(
                  ({
                    key,
                    labelKey,
                    hintKey,
                    percent,
                    Icon,
                    min,
                    max,
                    step,
                  }) => {
                    const value = percent
                      ? Number(pctDraft(draft[key] as number))
                      : (draft[key] as number);
                    const rangeMin =
                      key === "recommendCompactRatio"
                        ? Math.max(min, Math.ceil(draft.hardRatio * 100))
                        : min;
                    return (
                      <label
                        key={key}
                        className="prefs-context-field prefs-context-field--range"
                      >
                        <span className="prefs-context-field-copy">
                          <span className="prefs-context-label-row">
                            {Icon && (
                              <span
                                className="prefs-context-field-badge"
                                aria-hidden
                              >
                                <Icon size={12} strokeWidth={2.25} />
                              </span>
                            )}
                            <span className="prefs-context-label">
                              {t(labelKey)}
                            </span>
                          </span>
                          {hintKey && (
                            <span className="prefs-context-hint">
                              {t(hintKey)}
                            </span>
                          )}
                        </span>
                        <CompressionRange
                          label={t(labelKey)}
                          value={value}
                          min={rangeMin}
                          max={max}
                          step={step}
                          suffix={percent ? "%" : undefined}
                          onChange={(nextValue) =>
                            setField(
                              key,
                              (percent ? nextValue / 100 : nextValue) as never,
                            )
                          }
                          onCommit={(nextValue) => {
                            if (percent) {
                              commitRatio(
                                key as "midRunSummaryRatio",
                                String(nextValue),
                              );
                            } else {
                              commitInt(key, String(nextValue));
                            }
                          }}
                        />
                      </label>
                    );
                  },
                )}
              </div>
            </div>
          </details>

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
