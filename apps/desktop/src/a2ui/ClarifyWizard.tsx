/** 多题澄清叠层向导：Tab + 选项/自由输入 + 卡片切换动画。 */

import { useCallback, useEffect, useId, useRef, useState } from "react";
import {
  ArrowRight,
  Check,
  PencilLine,
  ShieldAlert,
  ShieldCheck,
  TerminalSquare,
  X,
} from "lucide-react";
import { useI18n } from "../i18n/LocaleContext";
import {
  isPresetAnswer,
  parseClarifySteps,
  parseApprovalContent,
  shouldSubmitClarifyInput,
  type ClarifyWizardStep,
} from "./clarifySteps";

export type { ClarifyWizardStep };
export { parseClarifySteps };

type Props = {
  steps: ClarifyWizardStep[];
  variant?: "default" | "approval";
  approvalTitle?: string;
  approvalBody?: string;
  approvalKind?: string;
  approvalDetail?: string;
  allowAlways?: boolean;
  approvalTypeLabel?: string;
  disabled?: boolean;
  onAction: (name: string, context: Record<string, unknown>) => void;
};

type Phase = "idle" | "exit" | "enter";
type Direction = "forward" | "backward";

export default function ClarifyWizard({
  steps,
  variant = "default",
  approvalTitle,
  approvalBody,
  approvalKind,
  approvalDetail,
  allowAlways = false,
  approvalTypeLabel,
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
  const [approvalChoice, setApprovalChoice] = useState<
    "approve" | "approve_always" | "approve_type" | "deny" | null
  >(null);
  const approvalTitleId = useId();
  const customInputRef = useRef<HTMLInputElement>(null);

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

  if (collapsed && variant === "approval" && approvalChoice) {
    const approved = approvalChoice !== "deny";
    const label =
      approvalChoice === "approve_always"
        ? t("chat.a2ui.approvalAlwaysApproved")
        : approvalChoice === "approve_type"
          ? t("chat.a2ui.approvalTypeApproved", {
              type: approvalTypeLabel ?? "",
            })
          : approved
            ? t("chat.a2ui.approvalApproved")
            : t("chat.a2ui.approvalDenied");
    return (
      <div
        className={`a2ui-clarify-wizard is-collapsed is-approval-result ${approved ? "is-approved" : "is-denied"}`}
        data-a2ui-id="wizard"
        role="status"
      >
        <div className="a2ui-approval-result">
          <span className="a2ui-approval-result-icon" aria-hidden>
            {approved ? (
              <Check size={15} strokeWidth={2.5} />
            ) : (
              <X size={15} strokeWidth={2.5} />
            )}
          </span>
          <span>{label}</span>
        </div>
      </div>
    );
  }

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
          <span className="a2ui-clarify-collapsed-icon" aria-hidden>
            ✓
          </span>
          <div className="a2ui-clarify-collapsed-body">
            {summaryParts.map((p) => (
              <span key={p.question} className="a2ui-clarify-collapsed-pair">
                <span className="a2ui-clarify-collapsed-q">{p.question}</span>
                <span className="a2ui-clarify-collapsed-a">{p.answer}</span>
              </span>
            ))}
          </div>
          <span className="a2ui-clarify-collapsed-expand" aria-hidden>
            ▸
          </span>
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
  const customActionLabel = isLast
    ? t("chat.a2ui.clarifySubmit")
    : t("chat.a2ui.clarifyNext");
  const previousStepsAnswered = steps
    .slice(0, safeIndex)
    .every((s) => answers[s.id]?.trim() || customDrafts[s.id]?.trim());
  const customActionDisabled =
    disabled ||
    phase === "exit" ||
    !currentHasAnswer() ||
    (isLast && !previousStepsAnswered);

  const handleCustomAction = () => {
    if (customActionDisabled) return;
    const draft = customDrafts[step.id]?.trim();
    if (draft) {
      confirmCustom(draft);
      return;
    }
    if (isSingle) return;
    if (isLast) {
      handleSubmit();
    } else {
      handleNext();
    }
  };

  const backPeek = multi
    ? steps.slice(safeIndex + 1, safeIndex + 3).map((_, i) => i + 1)
    : [];

  const dirClass = `is-${direction}`;

  if (variant === "approval") {
    const isSandboxRetry = approvalKind === "sandbox_retry";
    const content = isSandboxRetry
      ? {
          description: t("chat.a2ui.sandboxRetryDescription"),
          command: approvalDetail ?? "",
        }
      : parseApprovalContent(approvalBody ?? step.question);
    const renderedTitle = isSandboxRetry
      ? t("chat.a2ui.sandboxRetryTitle")
      : approvalTitle || step.question;
    const commandLabel = isSandboxRetry
      ? t("chat.a2ui.sandboxRetryDetail")
      : t("chat.a2ui.approvalCommand");
    const submitApproval = (
      choice: "approve" | "approve_always" | "approve_type" | "deny",
    ) => {
      if (disabled) return;
      setApprovalChoice(choice);
      setCollapsed(true);
      onAction(choice, {});
    };
    return (
      <section
        className={`a2ui-clarify-wizard is-approval ${disabled ? "is-disabled" : ""}`}
        data-a2ui-id="wizard"
        aria-labelledby={approvalTitleId}
      >
        <div className="a2ui-approval-header">
          <span className="a2ui-approval-mark" aria-hidden>
            <ShieldAlert size={19} strokeWidth={2} />
          </span>
          <div className="a2ui-approval-heading">
            <span className="a2ui-approval-eyebrow">
              {t("chat.a2ui.approvalRequired")}
            </span>
            <h3 id={approvalTitleId} className="a2ui-approval-title">
              {renderedTitle}
            </h3>
          </div>
        </div>

        {content.description ? (
          <p className="a2ui-approval-description">{content.description}</p>
        ) : null}

        {content.command ? (
          <div className="a2ui-approval-command">
            <div className="a2ui-approval-command-label">
              <TerminalSquare size={14} aria-hidden />
              <span>{commandLabel}</span>
            </div>
            <pre>
              <code>{content.command}</code>
            </pre>
          </div>
        ) : null}

        <div className="a2ui-approval-actions">
          <button
            type="button"
            className="a2ui-approval-action is-deny"
            disabled={disabled}
            onClick={() => submitApproval("deny")}
          >
            <X size={17} strokeWidth={2.2} aria-hidden />
            <span className="a2ui-approval-action-copy">
              <strong>{t("chat.a2ui.approvalDeny")}</strong>
              <small>{t("chat.a2ui.approvalDenyHint")}</small>
            </span>
          </button>
          <button
            type="button"
            className="a2ui-approval-action is-approve"
            disabled={disabled}
            onClick={() => submitApproval("approve")}
          >
            <Check size={17} strokeWidth={2.3} aria-hidden />
            <span className="a2ui-approval-action-copy">
              <strong>{t("chat.a2ui.approvalOnce")}</strong>
              <small>{t("chat.a2ui.approvalOnceHint")}</small>
            </span>
          </button>
        </div>

        {allowAlways ? (
          <div className="a2ui-approval-persistent-actions">
            <button
              type="button"
              className="a2ui-approval-always"
              disabled={disabled}
              onClick={() => submitApproval("approve_always")}
            >
              <ShieldCheck size={17} strokeWidth={2} aria-hidden />
              <span className="a2ui-approval-action-copy">
                <strong>{t("chat.a2ui.approvalAlways")}</strong>
                <small>{t("chat.a2ui.approvalAlwaysHint")}</small>
              </span>
              <ArrowRight
                className="a2ui-approval-always-arrow"
                size={16}
                aria-hidden
              />
            </button>
            {approvalTypeLabel ? (
              <button
                type="button"
                className="a2ui-approval-always is-type"
                disabled={disabled}
                onClick={() => submitApproval("approve_type")}
              >
                <TerminalSquare size={17} strokeWidth={2} aria-hidden />
                <span className="a2ui-approval-action-copy">
                  <strong>
                    {t("chat.a2ui.approvalType", { type: approvalTypeLabel })}
                  </strong>
                  <small>{t("chat.a2ui.approvalTypeHint")}</small>
                </span>
                <ArrowRight
                  className="a2ui-approval-always-arrow"
                  size={16}
                  aria-hidden
                />
              </button>
            ) : null}
          </div>
        ) : null}
      </section>
    );
  }

  const customInput = (
    <div
      className={`a2ui-clarify-custom ${hasPresets ? "is-inline-option" : "is-standalone"}`}
      onClick={(event) => {
        if ((event.target as HTMLElement).closest("button")) return;
        customInputRef.current?.focus();
      }}
    >
      <PencilLine className="a2ui-clarify-custom-icon" size={16} aria-hidden />
      <input
        ref={customInputRef}
        type="text"
        className="a2ui-clarify-custom-input"
        disabled={disabled || phase === "exit"}
        aria-label={t("chat.a2ui.clarifyCustom")}
        placeholder={
          hasPresets
            ? t("chat.a2ui.clarifyCustom")
            : t("chat.a2ui.clarifyCustomPlaceholder")
        }
        value={customValue}
        onChange={(e) => {
          const v = e.target.value;
          setCustomDrafts((prev) => ({ ...prev, [step.id]: v }));
          setAnswers((prev) => {
            const next = { ...prev };
            if (v.trim()) next[step.id] = v.trim();
            else delete next[step.id];
            return next;
          });
        }}
        onKeyDown={(e) => {
          if (shouldSubmitClarifyInput(e.key, e.nativeEvent.isComposing)) {
            e.preventDefault();
            handleCustomAction();
          }
        }}
      />
      <button
        type="button"
        className="a2ui-clarify-custom-submit"
        disabled={customActionDisabled}
        onClick={handleCustomAction}
        aria-label={customActionLabel}
        title={customActionLabel}
      >
        <ArrowRight size={17} strokeWidth={2.2} aria-hidden />
      </button>
    </div>
  );

  return (
    <div
      className={`a2ui-clarify-wizard ${disabled ? "is-disabled" : ""} ${multi ? "is-multi" : "is-single"}`}
      data-a2ui-id="wizard"
    >
      {multi ? (
        <div
          className="a2ui-clarify-tabs"
          role="tablist"
          aria-label={t("chat.a2ui.clarifyTabs")}
        >
          {steps.map((s, i) => {
            const answered = Boolean(
              answers[s.id]?.trim() || customDrafts[s.id]?.trim(),
            );
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
                  {s.question.length > 10
                    ? `${s.question.slice(0, 10)}…`
                    : s.question}
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
        </div>
      </div>
    </div>
  );
}
