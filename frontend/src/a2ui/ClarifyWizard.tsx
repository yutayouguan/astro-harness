/** 多题澄清叠层向导：Tab + 选完自动跳下一题 + 卡片切换动画。 */

import { useCallback, useEffect, useState } from "react";
import { useI18n } from "../i18n/LocaleContext";
import {
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

export default function ClarifyWizard({
  steps,
  disabled = false,
  onAction,
}: Props) {
  const { t } = useI18n();
  const [index, setIndex] = useState(0);
  const [answers, setAnswers] = useState<Record<string, string>>({});
  const [phase, setPhase] = useState<Phase>("idle");
  const [pendingIndex, setPendingIndex] = useState<number | null>(null);

  const safeIndex = Math.min(Math.max(index, 0), Math.max(steps.length - 1, 0));
  const step = steps[safeIndex];
  const isLast = safeIndex >= steps.length - 1;

  const goTo = useCallback(
    (next: number) => {
      if (disabled || steps.length === 0) return;
      const clamped = Math.min(Math.max(next, 0), steps.length - 1);
      if (clamped === safeIndex) return;
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
    const ms = reduce ? 0 : 220;
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
    const ms = reduce ? 0 : 240;
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
    },
    [onAction, steps],
  );

  const pick = useCallback(
    (value: string) => {
      if (disabled || !step || phase === "exit") return;
      const nextAnswers = { ...answers, [step.id]: value };
      setAnswers(nextAnswers);
      if (isLast) {
        submitAll(nextAnswers);
        return;
      }
      goTo(safeIndex + 1);
    },
    [answers, disabled, goTo, isLast, phase, safeIndex, step, submitAll],
  );

  if (!steps.length || !step) return null;

  const backPeek = steps
    .slice(safeIndex + 1, safeIndex + 3)
    .map((_, i) => i + 1);

  return (
    <div
      className={`a2ui-clarify-wizard ${disabled ? "is-disabled" : ""}`}
      data-a2ui-id="wizard"
    >
      <div className="a2ui-clarify-tabs" role="tablist" aria-label={t("chat.a2ui.clarifyTabs")}>
        {steps.map((s, i) => {
          const answered = Boolean(answers[s.id]);
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
            phase === "exit" ? "is-exit" : "",
            phase === "enter" ? "is-enter" : "",
          ]
            .filter(Boolean)
            .join(" ")}
          role="tabpanel"
        >
          <p className="a2ui-clarify-progress">
            {t("chat.a2ui.clarifyProgress")
              .replace("{current}", String(safeIndex + 1))
              .replace("{total}", String(steps.length))}
          </p>
          <h3 className="a2ui-clarify-question">{step.question}</h3>
          <div className="a2ui-clarify-options">
            {step.options.map((opt) => {
              const selected = answers[step.id] === opt;
              return (
                <button
                  key={opt}
                  type="button"
                  className={`a2ui-clarify-option ${selected ? "is-selected" : ""}`}
                  disabled={disabled || phase === "exit"}
                  onClick={() => pick(opt)}
                >
                  {opt}
                </button>
              );
            })}
          </div>
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
                  disabled || !answers[step.id] || phase === "exit"
                }
                onClick={() => goTo(safeIndex + 1)}
              >
                {t("chat.a2ui.clarifyNext")}
              </button>
            ) : (
              <button
                type="button"
                className="a2ui-button is-primary"
                disabled={
                  disabled ||
                  steps.some((s) => !answers[s.id]) ||
                  phase === "exit"
                }
                onClick={() => submitAll(answers)}
              >
                {t("chat.a2ui.clarifySubmit")}
              </button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
