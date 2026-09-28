/** 多题澄清叠层向导：Tab + 选项/自由输入 + 卡片切换动画。 */

import {
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
  type ReactNode,
} from "react";
import {
  ArrowRight,
  Check,
  Clock3,
  Copy,
  Globe2,
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
  /** 额外提供「本次会话允许」（网络授权等会话级权限）。 */
  allowSession?: boolean;
  approvalTypeLabel?: string;
  /** network 授权：目标主机（用于「始终允许 <host>」）。 */
  approvalHost?: string;
  /** network 授权：命中的 profile 名称。 */
  approvalProfile?: string;
  /** network 授权：触发连接的命令预览。 */
  approvalCommand?: string;
  /** 风险档位（`dangerous` / `sensitive`）：高风险会在批准前追加一道内联确认。 */
  approvalRisk?: string;
  disabled?: boolean;
  deferCommit?: boolean;
  initialDraft?: {
    answers: Record<string, string>;
    customDrafts: Record<string, string>;
    index: number;
  };
  onDraftChange?: (draft: {
    answers: Record<string, string>;
    customDrafts: Record<string, string>;
    index: number;
  }) => void;
  onAction: (name: string, context: Record<string, unknown>) => void;
};

type Phase = "idle" | "exit" | "enter";

/** 审批动作 id：与 `useChatSession.onUiAction` 的 payload 映射一一对应。 */
type ApprovalChoice =
  | "approve"
  | "approve_always"
  | "approve_type"
  | "allow_once"
  | "allow_session"
  | "allow_always"
  | "retry_with_command"
  | "deny";
type Direction = "forward" | "backward";

export default function ClarifyWizard({
  steps,
  variant = "default",
  approvalTitle,
  approvalBody,
  approvalKind,
  approvalDetail,
  allowAlways = false,
  allowSession = false,
  approvalTypeLabel,
  approvalHost,
  approvalProfile,
  approvalCommand,
  approvalRisk,
  disabled = false,
  deferCommit = false,
  initialDraft,
  onDraftChange,
  onAction,
}: Props) {
  const { t } = useI18n();
  const [index, setIndex] = useState(initialDraft?.index ?? 0);
  const [answers, setAnswers] = useState<Record<string, string>>(
    initialDraft?.answers ?? {},
  );
  const [customDrafts, setCustomDrafts] = useState<Record<string, string>>(
    initialDraft?.customDrafts ?? {},
  );
  // 高风险审批：把键盘用户带进卡片，但并不抢走输入框/编辑器的焦点。
  useEffect(() => {
    if (variant !== "approval" || approvalRisk !== "dangerous") return;
    const active = document.activeElement as HTMLElement | null;
    if (active?.closest("input, textarea, [contenteditable='true']")) return;
    approvalSectionRef.current?.focus();
  }, [variant, approvalRisk]);

  useEffect(() => {
    onDraftChange?.({ answers, customDrafts, index });
  }, [answers, customDrafts, index, onDraftChange]);
  const [phase, setPhase] = useState<Phase>("idle");
  const [direction, setDirection] = useState<Direction>("forward");
  const [pendingIndex, setPendingIndex] = useState<number | null>(null);
  const [collapsed, setCollapsed] = useState(false);
  const [approvalChoice, setApprovalChoice] = useState<ApprovalChoice | null>(
    null,
  );
  const [approvalCopied, setApprovalCopied] = useState(false);
  const [approveSecondThought, setApproveSecondThought] = useState(false);
  const [commandDraft, setCommandDraft] = useState<string | null>(null);
  const approvalSectionRef = useRef<HTMLElement>(null);

  // Esc 只关闭"二阶确认 / 命令编辑"（不代替拒绝）。挂在 document 上：
  // 二阶确认会把主按钮换成面板，焦点可能已经落到卡片之外。
  useEffect(() => {
    if (variant !== "approval") return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      if (approveSecondThought) {
        event.preventDefault();
        setApproveSecondThought(false);
        return;
      }
      if (commandDraft != null) {
        event.preventDefault();
        setCommandDraft(null);
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [variant, approveSecondThought, commandDraft]);
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
      if (!deferCommit) setCollapsed(true);
    },
    [onAction, steps, deferCommit],
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
    // 「编辑后重试」也是已处理态（会拒绝原请求并发出新命令），所以用绿勾。
    const approved = approvalChoice !== "deny";
    const label =
      approvalChoice === "retry_with_command"
        ? t("chat.a2ui.approvalRetryApproved")
        : approvalChoice === "approve_always" || approvalChoice === "allow_always"
        ? t("chat.a2ui.approvalAlwaysApproved")
        : approvalChoice === "approve_type"
          ? t("chat.a2ui.approvalTypeApproved", {
              type: approvalTypeLabel ?? "",
            })
          : approvalChoice === "allow_session"
            ? t("chat.a2ui.approvalSessionApproved")
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
    const isNetwork = approvalKind === "network";
    const authorization = isNetwork || isSandboxRetry;

    // 动作 id 与文案按语义分档：确认/沙箱提权用 approve*，网络授权用 allow*（会话级）。
    const actions = isNetwork
      ? {
          once: {
            id: "allow_once" as const,
            label: t("chat.a2ui.networkAllowOnce"),
            hint: t("chat.a2ui.networkAllowOnceHint"),
          },
          session: {
            id: "allow_session" as const,
            label: t("chat.a2ui.networkAllowSession"),
            hint: t("chat.a2ui.networkAllowSessionHint"),
          },
          always: {
            id: "allow_always" as const,
            label: t("chat.a2ui.networkAllowAlways", {
              host: approvalHost ?? approvalTypeLabel ?? "",
            }),
            hint: t("chat.a2ui.networkAllowAlwaysHint"),
          },
          deny: {
            id: "deny" as const,
            label: t("chat.a2ui.networkDeny"),
            hint: t("chat.a2ui.networkDenyHint"),
          },
        }
      : {
          once: {
            id: "approve" as const,
            label: t("chat.a2ui.approvalOnce"),
            hint: t("chat.a2ui.approvalOnceHint"),
          },
          always: {
            id: "approve_always" as const,
            label: t("chat.a2ui.approvalAlways"),
            hint: t("chat.a2ui.approvalAlwaysHint"),
          },
          type: {
            id: "approve_type" as const,
            label: t("chat.a2ui.approvalType", {
              type: approvalTypeLabel ?? "",
            }),
            hint: t("chat.a2ui.approvalTypeHint"),
          },
          deny: {
            id: "deny" as const,
            label: t("chat.a2ui.approvalDeny"),
            hint: t("chat.a2ui.approvalDenyHint"),
          },
        };

    const networkDescription = [
      approvalCommand
        ? t("chat.a2ui.networkDescriptionWithCommand", {
            command: approvalCommand,
          })
        : t("chat.a2ui.networkDescription"),
      approvalProfile
        ? t("chat.a2ui.networkProfile", { profile: approvalProfile })
        : null,
    ]
      .filter(Boolean)
      .join("\n");

    const content = isSandboxRetry
      ? approvalCommand
        ? {
            description: [
              t("chat.a2ui.sandboxRetryDescription"),
              approvalDetail ?? "",
            ]
              .filter(Boolean)
              .join("\n\n"),
            command: approvalCommand,
          }
        : {
            description: t("chat.a2ui.sandboxRetryDescription"),
            command: approvalDetail ?? "",
          }
      : isNetwork
        ? { description: networkDescription, command: approvalDetail ?? "" }
        : parseApprovalContent(approvalBody ?? step.question);
    const renderedTitle = isSandboxRetry
      ? t("chat.a2ui.sandboxRetryTitle")
      : isNetwork
        ? t("chat.a2ui.networkTitle")
        : approvalTitle || step.question;
    const commandText = content.command ?? "";
    const commandLabel = isSandboxRetry
      ? approvalCommand
        ? t("chat.a2ui.approvalCommand")
        : t("chat.a2ui.sandboxRetryDetail")
      : isNetwork
        ? t("chat.a2ui.networkTarget")
        : t("chat.a2ui.approvalCommand");

    const riskLabel =
      approvalRisk === "dangerous"
        ? t("chat.a2ui.approvalRiskDangerous")
        : approvalRisk === "sensitive"
          ? t("chat.a2ui.approvalRiskSensitive")
          : null;
    // 高风险操作：主批准按钮先落到一道内联确认，避免误击直接执行。
    const needsApproveConfirm = approvalRisk === "dangerous";

    // 长期/会话级动作：网络授权是 会话 → 主机，其余是 永久 → 同类命令。
    const persistentActions: {
      key: string;
      id: ApprovalChoice;
      label: string;
      hint: string;
      icon: ReactNode;
    }[] = [];
    if (isNetwork) {
      if (allowSession) {
        persistentActions.push({
          key: "session",
          id: "allow_session",
          label: t("chat.a2ui.networkAllowSession"),
          hint: t("chat.a2ui.networkAllowSessionHint"),
          icon: <Clock3 size={17} strokeWidth={2} aria-hidden />,
        });
      }
      if (allowAlways) {
        persistentActions.push({
          key: "always",
          id: "allow_always",
          label: t("chat.a2ui.networkAllowAlways", {
            host: approvalHost ?? approvalTypeLabel ?? "",
          }),
          hint: t("chat.a2ui.networkAllowAlwaysHint"),
          icon: <ShieldCheck size={17} strokeWidth={2} aria-hidden />,
        });
      }
    } else {
      if (allowAlways) {
        persistentActions.push({
          key: "always",
          id: "approve_always",
          label: t("chat.a2ui.approvalAlways"),
          hint: t("chat.a2ui.approvalAlwaysHint"),
          icon: <ShieldCheck size={17} strokeWidth={2} aria-hidden />,
        });
      }
      if (approvalTypeLabel) {
        persistentActions.push({
          key: "type",
          id: "approve_type",
          label: t("chat.a2ui.approvalType", { type: approvalTypeLabel }),
          hint: t("chat.a2ui.approvalTypeHint"),
          icon: <TerminalSquare size={17} strokeWidth={2} aria-hidden />,
        });
      }
    }

    const submitApproval = (choice: ApprovalChoice) => {
      if (disabled) return;
      setApprovalChoice(choice);
      setCollapsed(true);
      onAction(choice, {});
    };
    // 键盘收口：Esc 只关闭"二阶确认/命令编辑"，不代替拒绝；高风险操作必须
    // 点击或 ⌘/Ctrl+Enter 才批准（普通 Enter 不触发）。
    const onApprovalKeyDown = (event: React.KeyboardEvent<HTMLElement>) => {
      if (event.key !== "Enter") return;
      if (event.metaKey || event.ctrlKey) {
        event.preventDefault();
        if (approveSecondThought || !needsApproveConfirm) {
          submitApproval(actions.once.id);
        } else {
          setApproveSecondThought(true);
        }
        return;
      }
      if (
        approvalRisk === "dangerous" &&
        (event.target as HTMLElement | null)?.closest?.(
          ".a2ui-approval-action.is-approve",
        )
      ) {
        event.preventDefault();
      }
    };
    const copyCommand = () => {
      void navigator.clipboard
        ?.writeText(commandText)
        .then(() => {
          setApprovalCopied(true);
          window.setTimeout(() => setApprovalCopied(false), 1600);
        })
        .catch(() => undefined);
    };

    return (
      <section
        className={`a2ui-clarify-wizard is-approval${
          disabled ? " is-disabled" : ""
        }${isNetwork ? " is-network" : ""}${
          isSandboxRetry ? " is-sandbox-retry" : ""
        }`}
        data-a2ui-id="wizard"
        aria-labelledby={approvalTitleId}
        ref={approvalSectionRef}
        tabIndex={-1}
        onKeyDown={onApprovalKeyDown}
      >
        <div className="a2ui-approval-intro">
          <div className="a2ui-approval-header">
            <span className="a2ui-approval-mark" aria-hidden>
              {isNetwork ? (
                <Globe2 size={19} strokeWidth={2} />
              ) : (
                <ShieldAlert size={19} strokeWidth={2} />
              )}
            </span>
            <div className="a2ui-approval-heading">
              <span className="a2ui-approval-labels">
                <span className="a2ui-approval-eyebrow">
                  {authorization
                    ? t("chat.a2ui.approvalAuthorization")
                    : t("chat.a2ui.approvalRequired")}
                </span>
                {riskLabel ? (
                  <span
                    className={`a2ui-approval-risk is-${approvalRisk}`}
                    title={riskLabel}
                  >
                    {riskLabel}
                  </span>
                ) : null}
              </span>
              <h3 id={approvalTitleId} className="a2ui-approval-title">
                {renderedTitle}
              </h3>
            </div>
          </div>
          {content.description ? (
            <p className="a2ui-approval-description">{content.description}</p>
          ) : null}
        </div>

        {content.command ? (
          <div className="a2ui-approval-command">
            <div className="a2ui-approval-command-label">
              <TerminalSquare size={14} aria-hidden />
              <span>{commandLabel}</span>
              {isSandboxRetry ? (
                <button
                  type="button"
                  className="a2ui-approval-copy"
                  onClick={() =>
                    setCommandDraft((current) =>
                      current == null ? commandText : null,
                    )
                  }
                  aria-label={t("chat.a2ui.approvalEditCommand")}
                  title={t("chat.a2ui.approvalEditCommand")}
                >
                  <PencilLine size={13} strokeWidth={2} aria-hidden />
                </button>
              ) : null}
              <button
                type="button"
                className="a2ui-approval-copy"
                onClick={copyCommand}
                aria-label={t("chat.a2ui.approvalCopy")}
                title={t("chat.a2ui.approvalCopy")}
              >
                {approvalCopied ? (
                  <Check size={13} strokeWidth={2.4} aria-hidden />
                ) : (
                  <Copy size={13} strokeWidth={2} aria-hidden />
                )}
              </button>
            </div>
            {commandDraft != null ? (
              <div className="a2ui-approval-command-editor">
                <textarea
                  className="a2ui-approval-command-input"
                  value={commandDraft}
                  rows={3}
                  spellCheck={false}
                  aria-label={t("chat.a2ui.approvalEditCommand")}
                  onChange={(event) => setCommandDraft(event.target.value)}
                />
                <div className="a2ui-approval-command-edit-actions">
                  <button
                    type="button"
                    className="a2ui-approval-action is-ghost"
                    onClick={() => setCommandDraft(null)}
                  >
                    <span className="a2ui-approval-action-copy">
                      <strong>{t("chat.a2ui.approvalRetryCancel")}</strong>
                    </span>
                  </button>
                  <button
                    type="button"
                    className="a2ui-approval-action is-approve"
                    disabled={disabled || !commandDraft.trim()}
                    onClick={() => {
                      const command = commandDraft.trim();
                      if (!command) return;
                      setCommandDraft(null);
                      setApprovalChoice("retry_with_command");
                      setCollapsed(true);
                      onAction("retry_with_command", { command });
                    }}
                  >
                    <PencilLine size={16} strokeWidth={2.2} aria-hidden />
                    <span className="a2ui-approval-action-copy">
                      <strong>{t("chat.a2ui.approvalRetry")}</strong>
                      <small>{t("chat.a2ui.approvalRetryHint")}</small>
                    </span>
                  </button>
                </div>
              </div>
            ) : (
              <pre>
                <code>{commandText}</code>
              </pre>
            )}
          </div>
        ) : null}

        {isSandboxRetry && approvalCommand ? (
          <button
            type="button"
            className="a2ui-approval-open-terminal"
            onClick={() =>
              onAction("open_in_terminal", { command: approvalCommand })
            }
          >
            <TerminalSquare size={15} strokeWidth={2} aria-hidden />
            <span>{t("chat.a2ui.approvalOpenInTerminal")}</span>
            <ArrowRight
              className="a2ui-approval-always-arrow"
              size={16}
              aria-hidden
            />
          </button>
        ) : null}

        <div className="a2ui-approval-footer">
          {needsApproveConfirm && approveSecondThought ? (
            <div
              className="a2ui-approval-second-thoughts"
              role="group"
              aria-label={t("chat.a2ui.approvalRiskConfirmTitle")}
            >
              <p>{t("chat.a2ui.approvalRiskConfirmBody")}</p>
              <div className="a2ui-approval-second-thoughts-actions">
                <button
                  type="button"
                  className="a2ui-approval-action is-ghost"
                  onClick={() => setApproveSecondThought(false)}
                >
                  <span className="a2ui-approval-action-copy">
                    <strong>{t("chat.a2ui.approvalRiskBack")}</strong>
                  </span>
                </button>
                <button
                  type="button"
                  className="a2ui-approval-action is-approve"
                  disabled={disabled}
                  onClick={(event) => {
                    if (
                      event.detail === 0 &&
                      !event.metaKey &&
                      !event.ctrlKey
                    ) {
                      return;
                    }
                    submitApproval(actions.once.id);
                  }}
                >
                  <Check size={17} strokeWidth={2.3} aria-hidden />
                  <span className="a2ui-approval-action-copy">
                    <strong>{t("chat.a2ui.approvalRiskConfirm")}</strong>
                    <small>{actions.once.label}</small>
                  </span>
                </button>
              </div>
            </div>
          ) : (
            <div className="a2ui-approval-actions">
              <button
                type="button"
                className="a2ui-approval-action is-deny"
                disabled={disabled}
                onClick={() => submitApproval(actions.deny.id)}
              >
                <X size={17} strokeWidth={2.2} aria-hidden />
                <span className="a2ui-approval-action-copy">
                  <strong>{actions.deny.label}</strong>
                  <small>{actions.deny.hint}</small>
                </span>
              </button>
              <button
                type="button"
                className="a2ui-approval-action is-approve"
                disabled={disabled}
                onClick={(event) => {
                  // 高风险操作：键盘"回车/空格"这类 detail=0 的激活不算数，
                  // 必须显式点击（detail>=1）或 ⌘/Ctrl+Enter。
                  if (
                    needsApproveConfirm &&
                    event.detail === 0 &&
                    !event.metaKey &&
                    !event.ctrlKey
                  ) {
                    return;
                  }
                  if (needsApproveConfirm) setApproveSecondThought(true);
                  else submitApproval(actions.once.id);
                }}
                title={
                  needsApproveConfirm
                    ? t("chat.a2ui.approvalRiskShortcut")
                    : undefined
                }
              >
                <Check size={17} strokeWidth={2.3} aria-hidden />
                <span className="a2ui-approval-action-copy">
                  <strong>{actions.once.label}</strong>
                  <small>{actions.once.hint}</small>
                </span>
              </button>
            </div>
          )}

          {persistentActions.length > 0 ? (
            <div className="a2ui-approval-persistent-actions">
              {persistentActions.map((action) => (
                <button
                  key={action.key}
                  type="button"
                  className={`a2ui-approval-always is-${action.key}`}
                  disabled={disabled}
                  onClick={() => submitApproval(action.id)}
                >
                  {action.icon}
                  <span className="a2ui-approval-action-copy">
                    <strong>{action.label}</strong>
                    <small>{action.hint}</small>
                  </span>
                  <ArrowRight
                    className="a2ui-approval-always-arrow"
                    size={16}
                    aria-hidden
                  />
                </button>
              ))}
            </div>
          ) : null}
        </div>
      </section>
    );
  }

  const customInput = (
    <div
      className={`a2ui-clarify-custom ${hasPresets ? "is-inline-option" : "is-standalone"}`}
      data-input-surface
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
