/** 多题澄清叠层向导：Tab + 选项/自由输入 + 卡片切换动画。 */

import { useCallback, useEffect, useState } from "react";
import { useI18n } from "../i18n/LocaleContext";
import {
  isPresetAnswer,
  parseClarifySteps,
  type ClarifyWizardStep,
} from "./clarifySteps";

export type { ClarifyWizardStep };
export { parseClarifySteps };

type Props = {
  steps: ClarifyWizardStep[];
  disabled?: boolean;
  onAction: (name: string, context: Record<string, unknown>) => void;
};

type Phase = "idle" | "exit" | "enter";
type Direction = "forward" | "backward";

export default function ClarifyWizard({
  steps,
  disabled = false,
  onAction,
}: Props) {
  const { t } = useI18n();
  const [index, setIndex] = useState(0);
  const [answers, setAnswers] = useState<Record<string, string>>({});
  const [customDrafts, setCustomDrafts] = useState<Record<string, string>>({});
  const [phase, setPhase] = useState<Phase>("idle");
  const [direction, setDirection] = useState<Direction>("forward");
  const [pendingIndex, setPendingIndex] = useState<number | null>(null);
  const [collapsed, setCollapsed] = useState(false);

  const safeIndex = Math.min(Math.max(index, 0), Math.max(steps.length - 1, 0));
  const step = steps[safeIndex];
  const isLast = safeIndex >= steps.length - 1;
  const multi = steps.length > 1;
  const isSingle = !multi;

  const goTo = useCallback(
    (next: number) => {
      if (disabled || steps.length === 0) return;
      const clamped = Math.min(Math.max(next, 0), steps.length - 1);
      if (clamped === safeIndex) return;
      setDirection(clamped > safeIndex ? "forward" : "backward");
      setPendingIndex(clamped);
      setPhase("exit");
    },
    [disabled, safeIndex, steps.length],
  );

  useEffect(() => {
    if (phase !== "exit" || pendingIndex == null) return;
    const reduce =
      typeof window !== "undefined" &&
      window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const ms = reduce ? 0 : 180;
    const timer = window.setTimeout(() => {
      setIndex(pendingIndex);
      setPendingIndex(null);
      setPhase("enter");
    }, ms);
    return () => window.clearTimeout(timer);
  }, [phase, pendingIndex]);

  useEffect(() => {
    if (phase !== "enter") return;
    const reduce =
      typeof window !== "undefined" &&
      window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const ms = reduce ? 0 : 200;
    const timer = window.setTimeout(() => setPhase("idle"), ms);
    return () => window.clearTimeout(timer);
  }, [phase]);

  const submitAll = useCallback(
    (finalAnswers: Record<string, string>) => {
      const summary = steps
        .map((s) => {
          const a = finalAnswers[s.id];
          return a ? `${s.question}: ${a}` : null;
        })
        .filter(Boolean)
        .join("；");
      onAction("choose", {
        answers: finalAnswers,
        value: summary || Object.values(finalAnswers).join("；"),
      });
      setCollapsed(true);
    },
    [onAction, steps],
  );

  const advanceWithAnswer = useCallback(
    (nextAnswers: Record<string, string>) => {
      if (isSingle) {
        submitAll(nextAnswers);
        return;
      }
      if (!isLast) {
        goTo(safeIndex + 1);
      } else {
        const allAnswered = steps.every((s) => nextAnswers[s.id]?.trim());
        if (allAnswered) {
          submitAll(nextAnswers);
        }
      }
    },
    [goTo, isLast, isSingle, safeIndex, steps, submitAll],
  );

  const pickPreset = useCallback(
    (value: string) => {
      if (disabled || !step || phase === "exit") return;
      setCustomDrafts((prev) => {
        const next = { ...prev };
        delete next[step.id];
        return next;
      });
      const nextAnswers = { ...answers, [step.id]: value };
      setAnswers(nextAnswers);
      advanceWithAnswer(nextAnswers);
    },
    [advanceWithAnswer, answers, disabled, phase, step],
  );

  const confirmCustom = useCallback(
    (draft?: string) => {
      if (disabled || !step || phase === "exit") return;
      const raw = draft ?? customDrafts[step.id] ?? "";
      const value = raw.trim();
      if (!value) return;
      const nextAnswers = { ...answers, [step.id]: value };
      setAnswers(nextAnswers);
      setCustomDrafts((prev) => ({ ...prev, [step.id]: value }));
      advanceWithAnswer(nextAnswers);
    },
    [advanceWithAnswer, answers, customDrafts, disabled, phase, step],
  );

  /** 当前步骤是否有有效答案（含自定义草稿） */
  const currentHasAnswer = useCallback(() => {
    if (!step) return false;
    if (answers[step.id]?.trim()) return true;
    if (customDrafts[step.id]?.trim()) return true;
    return false;
  }, [answers, customDrafts, step]);

  /** 点"下一题"时：先保存自定义草稿（如有），再前进 */
  const handleNext = useCallback(() => {
    if (!step) return;
    const draft = customDrafts[step.id]?.trim();
    if (draft && !answers[step.id]?.trim()) {
      const nextAnswers = { ...answers, [step.id]: draft };
      setAnswers(nextAnswers);
      setCustomDrafts((prev) => ({ ...prev, [step.id]: draft }));
      advanceWithAnswer(nextAnswers);
    } else {
      goTo(safeIndex + 1);
    }
  }, [advanceWithAnswer, answers, customDrafts, goTo, safeIndex, step]);

  /** 点"提交"时：也要保存最后一题的自定义草稿 */
  const handleSubmit = useCallback(() => {
    let finalAnswers = { ...answers };
    if (step) {
      const draft = customDrafts[step.id]?.trim();
      if (draft && !finalAnswers[step.id]?.trim()) {
        finalAnswers = { ...finalAnswers, [step.id]: draft };
        setAnswers(finalAnswers);
      }
    }
    submitAll(finalAnswers);
  }, [answers, customDrafts, step, submitAll]);

  if (!steps.length || !step) return null;

  if (collapsed) {
    const summaryParts = steps
      .map((s) => {
        const a = answers[s.id];
        return a ? { question: s.question, answer: a } : null;
      })
      .filter(Boolean) as { question: string; answer: string }[];

    return (
      <div
        className="a2ui-clarify-wizard is-collapsed"
        data-a2ui-id="wizard"
        onClick={() => setCollapsed(false)}
        role="button"
        tabIndex={0}
        onKeyDown={(e) => e.key === "Enter" && setCollapsed(false)}
      >
        <div className="a2ui-clarify-collapsed">
          <span className="a2ui-clarify-collapsed-icon" aria-hidden>✓</span>
          <div className="a2ui-clarify-collapsed-body">
            {summaryParts.map((p) => (
              <span key={p.question} className="a2ui-clarify-collapsed-pair">
                <span className="a2ui-clarify-collapsed-q">{p.question}</span>
                <span className="a2ui-clarify-collapsed-a">{p.answer}</span>
              </span>
            ))}
          </div>
          <span className="a2ui-clarify-collapsed-expand" aria-hidden>▸</span>
        </div>
      </div>
    );
  }

  const hasPresets = step.options.length > 0;
  const saved = answers[step.id];
  const presetSelected = isPresetAnswer(step, saved) ? saved : undefined;
  const customValue =
    customDrafts[step.id] ??
    (saved && !isPresetAnswer(step, saved) ? saved : "");

  const backPeek = multi
    ? steps.slice(safeIndex + 1, safeIndex + 3).map((_, i) => i + 1)
    : [];

  const dirClass = `is-${direction}`;

  const customInput = (
    <label
      className={`a2ui-clarify-custom ${hasPresets ? "is-inline-option" : "is-standalone"}`}
    >
      {hasPresets ? (
        <span className="a2ui-clarify-custom-label">{t("chat.a2ui.clarifyCustom")}</span>
      ) : null}
      <input
        type="text"
        className="a2ui-clarify-custom-input"
        disabled={disabled || phase === "exit"}
        placeholder={t("chat.a2ui.clarifyCustomPlaceholder")}
        value={customValue}
        onChange={(e) => {
          const v = e.target.value;
          setCustomDrafts((prev) => ({ ...prev, [step.id]: v }));
          if (presetSelected) {
            setAnswers((prev) => {
              const next = { ...prev };
              delete next[step.id];
              return next;
            });
          }
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            confirmCustom();
          }
        }}
      />
      {!hasPresets || isSingle ? (
        <button
          type="button"
          className="a2ui-button is-primary a2ui-clarify-custom-submit"
          disabled={disabled || !customValue.trim() || phase === "exit"}
          onClick={() => confirmCustom()}
        >
          {isSingle ? t("chat.a2ui.clarifySubmit") : t("chat.a2ui.clarifyNext")}
        </button>
      ) : null}
    </label>
  );

  return (
    <div
      className={`a2ui-clarify-wizard ${disabled ? "is-disabled" : ""} ${multi ? "is-multi" : "is-single"}`}
      data-a2ui-id="wizard"
    >
      {multi ? (
        <div className="a2ui-clarify-tabs" role="tablist" aria-label={t("chat.a2ui.clarifyTabs")}>
          {steps.map((s, i) => {
            const answered = Boolean(answers[s.id]?.trim() || customDrafts[s.id]?.trim());
            const active = i === safeIndex;
            return (
              <button
                key={s.id}
                type="button"
                role="tab"
                aria-selected={active}
                className={[
                  "a2ui-clarify-tab",
                  active ? "is-active" : "",
                  answered ? "is-done" : "",
                ]
                  .filter(Boolean)
                  .join(" ")}
                disabled={disabled || phase === "exit"}
                onClick={() => goTo(i)}
              >
                <span className="a2ui-clarify-tab-index">{i + 1}</span>
                <span className="a2ui-clarify-tab-label">
                  {s.question.length > 10 ? `${s.question.slice(0, 10)}…` : s.question}
                </span>
              </button>
            );
          })}
        </div>
      ) : null}

      <div className="a2ui-clarify-stack" aria-live="polite">
        {backPeek.map((depth) => (
          <div
            key={`peek-${depth}`}
            className={`a2ui-clarify-layer is-peek is-depth-${depth}`}
            aria-hidden
          />
        ))}

        <div
          className={[
            "a2ui-clarify-layer is-front",
            phase === "exit" ? `is-exit ${dirClass}` : "",
            phase === "enter" ? `is-enter ${dirClass}` : "",
          ]
            .filter(Boolean)
            .join(" ")}
          role="tabpanel"
        >
          {multi ? (
            <div className="a2ui-clarify-progress">
              <span className="a2ui-clarify-progress-label">
                {t("chat.a2ui.clarifyProgress")
                  .replace("{current}", String(safeIndex + 1))
                  .replace("{total}", String(steps.length))}
              </span>
              <span className="a2ui-clarify-progress-track" aria-hidden>
                <span
                  className="a2ui-clarify-progress-fill"
                  style={{
                    transform: `scaleX(${(safeIndex + 1) / steps.length})`,
                  }}
                />
              </span>
            </div>
          ) : null}
          <h3 className="a2ui-clarify-question">{step.question}</h3>

          {hasPresets ? (
            <div className="a2ui-clarify-options">
              {step.options.map((opt) => {
                const selected = presetSelected === opt;
                return (
                  <button
                    key={opt}
                    type="button"
                    className={`a2ui-clarify-option ${selected ? "is-selected" : ""}`}
                    disabled={disabled || phase === "exit"}
                    onClick={() => pickPreset(opt)}
                  >
                    {opt}
                  </button>
                );
              })}
              {customInput}
            </div>
          ) : (
            customInput
          )}

          {multi ? (
            <div className="a2ui-clarify-nav">
              <button
                type="button"
                className="a2ui-button"
                disabled={disabled || safeIndex === 0 || phase === "exit"}
                onClick={() => goTo(safeIndex - 1)}
              >
                {t("chat.a2ui.clarifyBack")}
              </button>
              {!isLast ? (
                <button
                  type="button"
                  className="a2ui-button is-primary"
                  disabled={
                    disabled || !currentHasAnswer() || phase === "exit"
                  }
                  onClick={handleNext}
                >
                  {t("chat.a2ui.clarifyNext")}
                </button>
              ) : (
                <button
                  type="button"
                  className="a2ui-button is-primary"
                  disabled={
                    disabled ||
                    !currentHasAnswer() ||
                    steps.slice(0, -1).some((s) => !answers[s.id]?.trim()) ||
                    phase === "exit"
                  }
                  onClick={handleSubmit}
                >
                  {t("chat.a2ui.clarifySubmit")}
                </button>
              )}
            </div>
          ) : null}
        </div>
      </div>
    </div>
  );
}
