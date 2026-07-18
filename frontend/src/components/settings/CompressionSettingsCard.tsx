/** 偏好设置：上下文卫生（压缩阈值与保护参数）。 */
import { useCallback, useEffect, useState } from "react";
import { Layers3, RotateCcw } from "lucide-react";
import { useCompressionSettings } from "../../hooks/settings/useCompressionSettings";
import { useI18n } from "../../i18n/LocaleContext";
import type { MessageKey } from "../../i18n/messages";
import type { CompressionSettingsDto } from "../../types";

type Props = {
  tone?: string;
  active?: boolean;
};

type NumField = {
  key: keyof CompressionSettingsDto;
  labelKey: MessageKey;
  hintKey?: MessageKey;
  /** 界面用百分比展示（存 0–1） */
  percent?: boolean;
  step?: string;
  min?: number;
  max?: number;
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
      if (!(next.softRatio < next.mediumRatio && next.mediumRatio < next.hardRatio)) {
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

  const commitRatio = (key: "softRatio" | "mediumRatio" | "hardRatio" | "midRunSummaryRatio" | "recommendCompactRatio" | "thrashingMinGainRatio", raw: string) => {
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

  const stageRatioFields: NumField[] = [
    { key: "softRatio", labelKey: "prefs.context.soft", hintKey: "prefs.context.softDesc", percent: true },
    { key: "mediumRatio", labelKey: "prefs.context.medium", hintKey: "prefs.context.mediumDesc", percent: true },
    { key: "hardRatio", labelKey: "prefs.context.hard", hintKey: "prefs.context.hardDesc", percent: true },
  ];

  const budgetGroups: { titleKey: MessageKey; fields: NumField[] }[] = [
    {
      titleKey: "prefs.context.budgetSoft",
      fields: [
        { key: "softMaxChars", labelKey: "prefs.context.maxChars" },
        { key: "softHeadChars", labelKey: "prefs.context.headChars" },
        { key: "softTailChars", labelKey: "prefs.context.tailChars" },
      ],
    },
    {
      titleKey: "prefs.context.budgetMedium",
      fields: [
        { key: "mediumMaxChars", labelKey: "prefs.context.maxChars" },
        { key: "mediumHeadChars", labelKey: "prefs.context.headChars" },
        { key: "mediumTailChars", labelKey: "prefs.context.tailChars" },
      ],
    },
    {
      titleKey: "prefs.context.budgetHard",
      fields: [
        { key: "hardMaxChars", labelKey: "prefs.context.maxChars" },
        { key: "hardHeadChars", labelKey: "prefs.context.headChars" },
        { key: "hardTailChars", labelKey: "prefs.context.tailChars" },
      ],
    },
  ];

  const otherFields: NumField[] = [
    { key: "toolResultsLimit", labelKey: "prefs.context.toolResultsLimit", hintKey: "prefs.context.toolResultsLimitDesc" },
    { key: "midRunSummaryRatio", labelKey: "prefs.context.midRun", hintKey: "prefs.context.midRunDesc", percent: true },
    { key: "recommendCompactRatio", labelKey: "prefs.context.recommend", hintKey: "prefs.context.recommendDesc", percent: true },
    { key: "protectLastN", labelKey: "prefs.context.protectLast", hintKey: "prefs.context.protectLastDesc" },
    { key: "protectFirstMessages", labelKey: "prefs.context.protectFirst", hintKey: "prefs.context.protectFirstDesc" },
    { key: "thrashingMinGainRatio", labelKey: "prefs.context.thrashingGain", hintKey: "prefs.context.thrashingGainDesc", percent: true },
    { key: "thrashingMaxConsecutive", labelKey: "prefs.context.thrashingMax", hintKey: "prefs.context.thrashingMaxDesc" },
    { key: "keepTailBubbles", labelKey: "prefs.context.keepTail", hintKey: "prefs.context.keepTailDesc" },
  ];

  const displayError = localError ?? error;

  return (
    <section className="prefs-card">
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
            <span className="prefs-toggle-text">
              <span className="prefs-toggle-label">{t("prefs.context.enabled")}</span>
              <span className="prefs-toggle-desc">{t("prefs.context.enabledDesc")}</span>
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

          <h3 className="prefs-toggle-heading">{t("prefs.context.stages")}</h3>
          <div className="prefs-context-grid">
            {stageRatioFields.map(({ key, labelKey, hintKey }) => (
              <label key={key} className="prefs-context-field">
                <span className="prefs-context-label">{t(labelKey)}</span>
                {hintKey && <span className="prefs-context-hint">{t(hintKey)}</span>}
                <div className="prefs-context-input-wrap">
                  <input
                    className="aux-number-input"
                    type="number"
                    min={5}
                    max={95}
                    step={1}
                    value={pctDraft(draft[key] as number)}
                    onChange={(e) => {
                      const p = parsePct(e.target.value);
                      if (p != null) setField(key, p as never);
                    }}
                    onBlur={(e) => commitRatio(key as "softRatio", e.target.value)}
                  />
                  <span className="prefs-context-suffix">%</span>
                </div>
              </label>
            ))}
          </div>

          <h3 className="prefs-toggle-heading">{t("prefs.context.budgets")}</h3>
          {budgetGroups.map((group) => (
            <div key={group.titleKey} className="prefs-context-budget-block">
              <h4 className="prefs-context-budget-title">{t(group.titleKey)}</h4>
              <div className="prefs-context-grid prefs-context-grid--3">
                {group.fields.map(({ key, labelKey }) => (
                  <label key={key} className="prefs-context-field">
                    <span className="prefs-context-label">{t(labelKey)}</span>
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
          ))}

          <h3 className="prefs-toggle-heading">{t("prefs.context.triggers")}</h3>
          <div className="prefs-context-grid">
            {otherFields.map(({ key, labelKey, hintKey, percent }) => (
              <label key={key} className="prefs-context-field">
                <span className="prefs-context-label">{t(labelKey)}</span>
                {hintKey && <span className="prefs-context-hint">{t(hintKey)}</span>}
                <div className="prefs-context-input-wrap">
                  <input
                    className="aux-number-input"
                    type="number"
                    min={0}
                    step={percent ? 1 : 1}
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
