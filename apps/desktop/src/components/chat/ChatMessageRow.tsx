/** 单条消息行：抽自 ChatView，props 稳定时跳过流式重渲染。 */
import { memo, useEffect, useRef, useState, type ReactNode } from "react";
import {
  ChevronDown,
  GitBranch,
  MoreHorizontal,
  Pencil,
  RefreshCw,
  X,
} from "lucide-react";
import { Check as CheckData, Copy as CopyData } from "lucide";
import ClarifyWizard from "../../a2ui/ClarifyWizard";
import { useI18n } from "../../i18n/LocaleContext";
import { type AgentIconInfo } from "../../lib/agent/agentIcons";
import { projectCanonicalTimelineSegments } from "../../lib/chat/chatTimeline";
import { isClarifySurface } from "../../lib/chat/composerClarify";
import {
  groupConsecutiveActivities,
  isConsecutiveActivityGroup,
} from "../../lib/chat/groupActivities";
import { groupAssistantAnswer } from "../../lib/chat/groupAssistantAnswer";
import { isLocationRequiredSurface } from "../../lib/chat/locationSurface";
import {
  isTodoActivity,
  isTodoOnlyActivityMessage,
  type FileChangeItem,
} from "../../lib/chat/taskProgress";
import { isLiveActivityStatus } from "../../lib/chat/toolActivityStatus";
import {
  formatCompactTurnTokens,
  formatExactTurnTokens,
  formatTurnDuration,
} from "../../lib/chat/turnUsageDisplay";
import {
  isActivityVisible,
  type ChatAnswerLayout,
  type ChatDisplayPrefs,
} from "../../hooks/chat/useChatDisplayPrefs";
import type { ChatSendMode } from "../../lib/chat/chatMode";
import type {
  AsyncUserInputQuestion,
  ChatActivity,
  ChatAttachment,
  ConversationEntry,
  TurnTokenUsage,
} from "../../types";
import AgentAvatar from "../agents/AgentAvatar";
import { MorphToggleIcon } from "../icons/MorphIcon";
import { ModelBrandIcon } from "../icons/ProviderIcons";
import A2UISurfaceCard from "./A2UISurfaceCard";
import { AttachmentGlyph, formatSize } from "./attachmentDisplay";
import { ChatMarkdown } from "./ChatMarkdown";
import LocationA2UISurface from "./LocationA2UISurface";
import MsgActivity from "./MsgActivity";
import MsgActivityGroup from "./MsgActivityGroup";
import MsgCitations from "./MsgCitations";
import MsgReasoning from "./MsgReasoning";
import MsgStreamLoader from "./MsgStreamLoader";
import {
  MsgTimeline,
  MsgTimelineStep,
  type MsgTimelineKind,
} from "./MsgTimeline";
import TurnChangeSummaryCard from "./TurnChangeSummaryCard";

export type ChatMessageRowProps = {
  message: ConversationEntry;
  /** 会话最后一条 / 并行执行中：决定该行是否按“流式中”渲染。 */
  isLastMessage: boolean;
  isParallelRunning: boolean;
  /** 该行是否是可编辑的最后一条用户消息。 */
  isLastUserMessage: boolean;
  displayPrefs: ChatDisplayPrefs;
  answerLayout: ChatAnswerLayout;
  forcedProcessOpen?: boolean;
  streaming: boolean;
  turnInFlight: boolean;
  sendBlocked: boolean;
  isEditingUserMessage: boolean;
  editingUserDraft: string;
  submittingUserEdit: boolean;
  pendingAsyncQuestions?: AsyncUserInputQuestion[];
  activeAgent: (AgentIconInfo & { path?: string }) | null;
  assistantHasCustomAvatar: boolean;
  modelId: string | null;
  mediaBaseDir?: string | null;
  projectId?: string | null;
  /** 交互回调：ChatView 侧均为稳定引用（useCallback）。 */
  openAssistantTurnMenu: (
    messageId: string,
    x: number,
    y: number,
    respectTextSelection?: boolean,
    returnFocus?: HTMLButtonElement | null,
  ) => boolean;
  setEditingUserDraft: (value: string) => void;
  beginUserMessageEdit: (message: ConversationEntry) => void;
  cancelUserMessageEdit: () => void;
  submitUserMessageEdit: () => void | Promise<void>;
  onEditUserMessage?: (messageId: string, content: string) => Promise<boolean>;
  onBranchMessage?: (messageId: string) => void;
  onOpenActivityUrl?: (url: string) => void | Promise<void>;
  onOpenFileReview?: (file: FileChangeItem, files: FileChangeItem[]) => void;
  onUiAction?: (
    messageId: string,
    name: string,
    context: Record<string, unknown>,
  ) => void;
  onSend: (opts?: { text?: string; sendMode?: ChatSendMode }) => void;
};

const CHECK_ICON = CheckData;
const COPY_ICON = CopyData;

function formatTokenSpeed(n: number): string {
  return Number.isInteger(n) ? String(n) : n.toFixed(1);
}

export function MessageTokenStats({
  usage,
  tokensPerSec,
  generationDurationSec,
}: {
  usage?: TurnTokenUsage;
  tokensPerSec?: number;
  generationDurationSec?: number;
}) {
  const { locale, t } = useI18n();
  const hasUsage = Boolean(
    usage &&
      (usage.totalTokens || usage.promptTokens || usage.completionTokens),
  );
  const hasDuration = generationDurationSec != null;
  if (!hasUsage && !hasDuration) return null;

  const speed =
    tokensPerSec != null && tokensPerSec > 0
      ? formatTokenSpeed(tokensPerSec)
      : null;
  const total = hasUsage
    ? formatCompactTurnTokens(usage!.totalTokens, locale)
    : null;
  const durationLabel = hasDuration
    ? t("chat.generationDuration", {
        s: formatTurnDuration(generationDurationSec!, locale),
      })
    : null;
  const cacheHit =
    usage?.cacheReadReported && usage.promptTokens > 0
      ? Math.min(
          100,
          Math.round((usage.cacheReadTokens / usage.promptTokens) * 100),
        )
      : null;
  const aria = t("chat.tokenStatsAria", {
    total: formatExactTurnTokens(usage?.totalTokens ?? 0, locale),
    prompt: formatExactTurnTokens(usage?.promptTokens ?? 0, locale),
    completion: formatExactTurnTokens(usage?.completionTokens ?? 0, locale),
  });

  if (!hasUsage) {
    return (
      <div className="msg-token-stats is-static">
        <span className="msg-token-stats-duration">{durationLabel}</span>
      </div>
    );
  }

  return (
    <details className="msg-token-stats" aria-label={aria}>
      <summary
        className="msg-token-stats-summary"
        title={t("chat.tokenDetails")}
      >
        {durationLabel ? (
          <span className="msg-token-stats-duration">{durationLabel}</span>
        ) : null}
        <span className="msg-token-stats-usage">
          {t("chat.tokenSummary", { total: total ?? "0" })}
        </span>
        {cacheHit != null ? (
          <span className="msg-token-stats-cache">
            {t("chat.tokenCacheSummary", { pct: String(cacheHit) })}
          </span>
        ) : null}
        <ChevronDown
          className="msg-token-stats-chevron"
          size={12}
          strokeWidth={2}
          aria-hidden
        />
      </summary>
      <div className="msg-token-stats-details">
        <span>
          {t("chat.tokenInput", {
            tokens: formatCompactTurnTokens(usage!.promptTokens, locale),
          })}
        </span>
        <span>
          {t("chat.tokenOutput", {
            tokens: formatCompactTurnTokens(usage!.completionTokens, locale),
          })}
        </span>
        {cacheHit != null ? (
          <span>
            {t("chat.tokenCacheHit", {
              tokens: formatCompactTurnTokens(usage!.cacheReadTokens, locale),
              pct: String(cacheHit),
            })}
          </span>
        ) : null}
        {usage?.reasoningReported ? (
          <span>
            {t("chat.tokenReasoning", {
              tokens: formatCompactTurnTokens(usage.reasoningTokens, locale),
            })}
          </span>
        ) : null}
        {speed != null ? (
          <span>{t("chat.tokenSpeed", { n: speed })}</span>
        ) : null}
      </div>
    </details>
  );
}

function MessageAttachments({ items }: { items: ChatAttachment[] }) {
  const { t } = useI18n();
  if (!items.length) return null;
  return (
    <div className="msg-attachments">
      {items.map((att) => (
        <div key={att.id} className="msg-attachment" data-kind={att.kind}>
          {att.kind === "image" && att.previewUrl ? (
            <img
              src={att.previewUrl}
              alt={att.name}
              className="msg-attachment-thumb"
            />
          ) : att.kind === "video" && att.previewUrl ? (
            <video
              src={att.previewUrl}
              className="msg-attachment-thumb"
              muted
            />
          ) : (
            <span className="msg-attachment-icon" data-kind={att.kind}>
              <AttachmentGlyph kind={att.kind} />
            </span>
          )}
          <div className="msg-attachment-meta">
            <span className="msg-attachment-name">{att.name}</span>
            <span className="msg-attachment-size">
              {att.kind === "folder"
                ? t("chat.plusMenuFolder")
                : formatSize(att.size)}
            </span>
          </div>
        </div>
      ))}
    </div>
  );
}

export function MessageActions({
  messageId,
  content,
  role,
  disabled,
  onEdit,
  onBranch,
  onOpenMenu,
}: {
  messageId: string;
  content: string;
  role: "user" | "assistant";
  disabled?: boolean;
  onEdit?: (messageId: string) => void;
  onBranch?: (messageId: string) => void;
  onOpenMenu?: (anchor: HTMLButtonElement) => void;
}) {
  const { t } = useI18n();
  const [copied, setCopied] = useState(false);

  const onCopy = async () => {
    if (!content) return;
    try {
      await navigator.clipboard.writeText(content);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1600);
    } catch {
      // ignore
    }
  };

  return (
    <div
      className="msg-actions"
      role="toolbar"
      aria-label={t("chat.messageActions")}
    >
      <button
        type="button"
        className={`msg-action-btn ${copied ? "is-copied" : ""}`}
        disabled={disabled || !content}
        onClick={() => void onCopy()}
        aria-label={copied ? t("chat.copied") : t("chat.copy")}
        title={copied ? t("chat.copied") : t("chat.copy")}
      >
        <MorphToggleIcon
          active={copied}
          activeIcon={CHECK_ICON}
          inactiveIcon={COPY_ICON}
          size={14}
          strokeWidth={copied ? 2.4 : 2}
          aria-hidden
        />
      </button>
      {role === "assistant" ? (
        <>
          <button
            type="button"
            className="msg-action-btn"
            disabled={disabled || !onBranch}
            onClick={() => onBranch?.(messageId)}
            aria-label={t("chat.branch")}
            title={t("chat.branchHint")}
          >
            <GitBranch size={14} strokeWidth={2} aria-hidden />
          </button>
          <button
            type="button"
            className="msg-action-btn"
            disabled={disabled || !onOpenMenu}
            onClick={(event) => onOpenMenu?.(event.currentTarget)}
            aria-label={t("chat.messageMenu.title")}
            title={t("chat.messageMenu.title")}
            aria-haspopup="menu"
          >
            <MoreHorizontal size={14} strokeWidth={2} aria-hidden />
          </button>
        </>
      ) : (
        <button
          type="button"
          className="msg-action-btn"
          disabled={disabled || !onEdit}
          onClick={() => onEdit?.(messageId)}
          aria-label={t("chat.editQuestion")}
          title={t("chat.editQuestion")}
        >
          <Pencil size={14} strokeWidth={2} aria-hidden />
        </button>
      )}
    </div>
  );
}

function InlineUserMessageEditor({
  value,
  originalValue,
  disabled,
  onChange,
  onCancel,
  onSubmit,
}: {
  value: string;
  originalValue: string;
  disabled: boolean;
  onChange: (value: string) => void;
  onCancel: () => void;
  onSubmit: () => void;
}) {
  const { t } = useI18n();
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const canSubmit =
    !disabled &&
    value.trim().length > 0 &&
    value.trim() !== originalValue.trim();

  useEffect(() => {
    const textarea = textareaRef.current;
    if (!textarea) return;
    textarea.focus();
    textarea.select();
  }, []);

  useEffect(() => {
    const textarea = textareaRef.current;
    if (!textarea) return;
    textarea.style.height = "0px";
    textarea.style.height = `${Math.min(textarea.scrollHeight, 180)}px`;
  }, [value]);

  return (
    <div className="user-message-editor">
      <textarea
        ref={textareaRef}
        className="user-message-editor-input"
        value={value}
        disabled={disabled}
        aria-label={t("chat.editQuestion")}
        onChange={(event) => onChange(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            event.preventDefault();
            onCancel();
            return;
          }
          if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
            event.preventDefault();
            if (canSubmit) onSubmit();
          }
        }}
      />
      <div className="user-message-editor-actions">
        <button
          type="button"
          className="user-message-editor-btn is-cancel"
          disabled={disabled}
          onClick={onCancel}
        >
          <X size={14} strokeWidth={2} aria-hidden />
          {t("chat.editCancel")}
        </button>
        <button
          type="button"
          className="user-message-editor-btn is-submit"
          disabled={!canSubmit}
          onClick={onSubmit}
        >
          <RefreshCw size={14} strokeWidth={2} aria-hidden />
          {t("chat.editSubmit")}
        </button>
      </div>
    </div>
  );
}

function ChatMessageRowImpl({
  message: m,
  isLastMessage,
  isParallelRunning,
  isLastUserMessage,
  displayPrefs,
  answerLayout,
  forcedProcessOpen,
  streaming,
  turnInFlight,
  sendBlocked,
  isEditingUserMessage,
  editingUserDraft,
  submittingUserEdit,
  pendingAsyncQuestions,
  activeAgent,
  assistantHasCustomAvatar,
  modelId,
  mediaBaseDir,
  projectId,
  openAssistantTurnMenu,
  setEditingUserDraft,
  beginUserMessageEdit,
  cancelUserMessageEdit,
  submitUserMessageEdit,
  onEditUserMessage,
  onBranchMessage,
  onOpenActivityUrl,
  onOpenFileReview,
  onUiAction,
  onSend,
}: ChatMessageRowProps) {
  const isStreamingBubble =
    m.role === "assistant" &&
    !m.error &&
    (isParallelRunning || (streaming && isLastMessage));
  const reasoningActive = Boolean(
    isStreamingBubble && m.reasoning && !m.content,
  );
  const canEditUserMessage =
    m.role === "user" &&
    isLastUserMessage &&
    !streaming &&
    !turnInFlight &&
    !sendBlocked &&
    Boolean(onEditUserMessage);
  if (isTodoOnlyActivityMessage(m)) return null;
  return (
    <div
      key={m.id}
      id={`msg-${m.id}`}
      data-msg-id={m.id}
      className={`msg-row ${m.role === "user" ? "user" : "assistant"}`}
    >
      {m.role === "assistant" && (
        <div
          className={`avatar ${m.error ? "error" : ""}${
            !m.error && !assistantHasCustomAvatar && modelId ? " is-model" : ""
          }`}
        >
          {m.error ? (
            "!"
          ) : assistantHasCustomAvatar && activeAgent ? (
            <AgentAvatar agent={activeAgent} size={20} />
          ) : modelId ? (
            <ModelBrandIcon modelId={modelId} size={16} />
          ) : (
            "AI"
          )}
        </div>
      )}
      <div className="msg-stack">
        <div
          className={`bubble ${m.role} ${m.error ? "error" : ""} ${
            !m.content &&
            !m.reasoning &&
            !m.attachments?.length &&
            !m.activities?.length &&
            !m.uiSurfaces?.length &&
            streaming
              ? "typing"
              : ""
          } ${isStreamingBubble && (m.content || m.reasoning) ? "is-streaming" : ""}${
            isEditingUserMessage ? " is-editing" : ""
          }`}
          data-allow-context-menu={m.role === "assistant" ? "true" : undefined}
          onContextMenu={(event) => {
            if (m.role !== "assistant" || isStreamingBubble) return;
            const opened = openAssistantTurnMenu(
              m.id,
              event.clientX,
              event.clientY,
            );
            if (!opened) return;
            event.preventDefault();
            event.stopPropagation();
          }}
        >
          {m.attachments && m.attachments.length > 0 && (
            <MessageAttachments items={m.attachments} />
          )}
          {isEditingUserMessage ? (
            <InlineUserMessageEditor
              value={editingUserDraft}
              originalValue={m.content}
              disabled={submittingUserEdit}
              onChange={setEditingUserDraft}
              onCancel={cancelUserMessageEdit}
              onSubmit={() => void submitUserMessageEdit()}
            />
          ) : (
            (() => {
              type Step = {
                key: string;
                kind: MsgTimelineKind;
                active?: boolean;
                activity?: ChatActivity;
                node: ReactNode;
              };
              const steps: Step[] = [];
              if (pendingAsyncQuestions) {
                steps.push({
                  key: `async-questions-${m.id}`,
                  kind: "surface",
                  active: true,
                  node: (
                    <ClarifyWizard
                      steps={pendingAsyncQuestions.map(
                        (question, questionIndex) => ({
                          id: `${m.id}-q${questionIndex}`,
                          question: question.title,
                          options: question.options ?? [],
                        }),
                      )}
                      disabled={sendBlocked}
                      onAction={(name, context) => {
                        if (
                          name === "choose" &&
                          typeof context.value === "string" &&
                          context.value.trim()
                        ) {
                          onSend({ text: context.value.trim() });
                        }
                      }}
                    />
                  ),
                });
              }
              const pushActivity = (act: ChatActivity) => {
                if (isTodoActivity(act)) return;
                if (!isActivityVisible(act.kind, displayPrefs)) return;
                steps.push({
                  key: `act-${act.id}`,
                  kind: act.kind,
                  active: isLiveActivityStatus(act.status),
                  activity: act,
                  node: (
                    <MsgActivity
                      activity={act}
                      defaultOpen={
                        forcedProcessOpen ?? displayPrefs.processDefaultOpen
                      }
                      showTimestamp={displayPrefs.showTimestamps}
                      mediaBaseDir={mediaBaseDir}
                      onOpenUrl={onOpenActivityUrl}
                    />
                  ),
                });
              };
              const pushSurface = (
                surface: NonNullable<ConversationEntry["uiSurfaces"]>[number],
              ) => {
                // Clarify is an input interaction: it belongs in the composer,
                // never inside the assistant answer timeline.
                if (isClarifySurface(surface)) return;
                steps.push({
                  key: `surf-${surface.messageId}`,
                  kind: "surface",
                  active: surface.status === "active",
                  node: isLocationRequiredSurface(surface) ? (
                    <LocationA2UISurface
                      operations={surface.operations}
                      disabled={surface.status !== "active"}
                      mediaBaseDir={mediaBaseDir}
                      onAction={(name, context) =>
                        onUiAction?.(m.id, name, context)
                      }
                    />
                  ) : (
                    <A2UISurfaceCard
                      surface={surface}
                      mediaBaseDir={mediaBaseDir}
                      onAction={(name, context) =>
                        onUiAction?.(m.id, name, context)
                      }
                    />
                  ),
                });
              };
              let hasTimelineText = Boolean(pendingAsyncQuestions);
              const timelineSegments =
                answerLayout === "timeline"
                  ? projectCanonicalTimelineSegments(m)
                  : m.segments;
              const reasoningOutcome: "done" | "error" | "interrupted" =
                m.turnStatus === "error" || m.error
                  ? "error"
                  : m.turnStatus === "interrupted"
                    ? "interrupted"
                    : "done";
              const lastReasoningSegmentId = timelineSegments
                ? [...timelineSegments]
                    .reverse()
                    .find((segment) => segment.type === "reasoning")?.id
                : undefined;

              if (timelineSegments && timelineSegments.length > 0) {
                for (const [segmentIndex, seg] of timelineSegments.entries()) {
                  if (seg.type === "reasoning") {
                    const openReasoning =
                      seg.durationSec == null || seg.durationSec <= 0;
                    const active = Boolean(isStreamingBubble && openReasoning);
                    steps.push({
                      key: seg.id,
                      kind: "reasoning",
                      active,
                      node: (
                        <MsgReasoning
                          reasoning={seg.text}
                          active={active}
                          outcome={
                            !active && seg.id === lastReasoningSegmentId
                              ? reasoningOutcome
                              : "done"
                          }
                          durationSec={seg.durationSec}
                          startedAtMs={active ? seg.at : undefined}
                          defaultOpen={displayPrefs.processDefaultOpen}
                          forcedOpen={forcedProcessOpen}
                        />
                      ),
                    });
                    continue;
                  }
                  if (seg.type === "text") {
                    hasTimelineText = true;
                    const active = Boolean(
                      isStreamingBubble &&
                        segmentIndex === timelineSegments.length - 1,
                    );
                    steps.push({
                      key: seg.id,
                      kind: "reply",
                      active,
                      node: (
                        <>
                          <ChatMarkdown
                            content={seg.text}
                            streaming={active}
                            compact={displayPrefs.verbosity === "compact"}
                            plain={Boolean(m.error)}
                            caret={false}
                            mediaBaseDir={mediaBaseDir}
                          />
                          <MsgStreamLoader visible={active} />
                        </>
                      ),
                    });
                    continue;
                  }
                  if (seg.type === "activity") {
                    const act = m.activities?.find((a) => a.id === seg.id);
                    if (act) pushActivity(act);
                    continue;
                  }
                  const surface = m.uiSurfaces?.find(
                    (s) => s.messageId === seg.id,
                  );
                  if (surface) pushSurface(surface);
                }
              } else {
                if (m.reasoning) {
                  steps.push({
                    key: `r-${m.id}`,
                    kind: "reasoning",
                    active: reasoningActive,
                    node: (
                      <MsgReasoning
                        reasoning={m.reasoning}
                        active={reasoningActive}
                        outcome={reasoningOutcome}
                        durationSec={m.reasoningDurationSec}
                        defaultOpen={displayPrefs.processDefaultOpen}
                        forcedOpen={forcedProcessOpen}
                      />
                    ),
                  });
                }
                for (const act of m.activities ?? []) {
                  pushActivity(act);
                }
                for (const surface of m.uiSurfaces ?? []) {
                  pushSurface(surface);
                }
                if (m.citations?.length) {
                  steps.push({
                    key: `cite-${m.id}`,
                    kind: "reasoning" as MsgTimelineKind,
                    active: false,
                    node: <MsgCitations citations={m.citations} />,
                  });
                }
              }

              if (answerLayout === "grouped") {
                const groupedAnswer = groupAssistantAnswer(m);
                const groupedViewSteps: Step[] = [];
                const lastSegment = m.segments?.[m.segments.length - 1];
                const groupedReasoningActive = Boolean(
                  isStreamingBubble &&
                    (lastSegment
                      ? lastSegment.type === "reasoning"
                      : reasoningActive),
                );

                if (groupedAnswer.reasoning) {
                  groupedViewSteps.push({
                    key: `grouped-reasoning-${m.id}`,
                    kind: "reasoning",
                    active: groupedReasoningActive,
                    node: (
                      <MsgReasoning
                        reasoning={groupedAnswer.reasoning}
                        active={groupedReasoningActive}
                        outcome={reasoningOutcome}
                        durationSec={groupedAnswer.reasoningDurationSec}
                        defaultOpen={displayPrefs.processDefaultOpen}
                        forcedOpen={forcedProcessOpen}
                      />
                    ),
                  });
                }

                const visibleActivities = (m.activities ?? []).filter(
                  (activity) =>
                    !isTodoActivity(activity) &&
                    isActivityVisible(activity.kind, displayPrefs),
                );
                if (visibleActivities.length > 0) {
                  groupedViewSteps.push({
                    key: `grouped-activities-${m.id}`,
                    kind: visibleActivities[0]?.kind ?? "tool",
                    active: visibleActivities.some((activity) =>
                      isLiveActivityStatus(activity.status),
                    ),
                    node: (
                      <MsgActivityGroup
                        activities={visibleActivities}
                        defaultOpen={displayPrefs.processDefaultOpen}
                        forcedOpen={forcedProcessOpen}
                        showTimestamp={displayPrefs.showTimestamps}
                        mediaBaseDir={mediaBaseDir}
                        onOpenUrl={onOpenActivityUrl}
                      />
                    ),
                  });
                }

                groupedViewSteps.push(
                  ...steps.filter((step) => step.kind === "surface"),
                );

                if (groupedAnswer.text && !pendingAsyncQuestions) {
                  groupedViewSteps.push({
                    key: `grouped-reply-${m.id}`,
                    kind: "reply",
                    active: isStreamingBubble,
                    node: (
                      <>
                        <ChatMarkdown
                          content={groupedAnswer.text}
                          streaming={isStreamingBubble}
                          compact={displayPrefs.verbosity === "compact"}
                          plain={Boolean(m.error)}
                          caret={false}
                          mediaBaseDir={mediaBaseDir}
                        />
                        <MsgStreamLoader visible={isStreamingBubble} />
                      </>
                    ),
                  });
                }

                if (m.citations?.length) {
                  groupedViewSteps.push({
                    key: `grouped-citations-${m.id}`,
                    kind: "reasoning",
                    active: false,
                    node: <MsgCitations citations={m.citations} />,
                  });
                }

                steps.splice(0, steps.length, ...groupedViewSteps);
                hasTimelineText = Boolean(
                  groupedAnswer.text || pendingAsyncQuestions,
                );
              }

              const showLoaderAlone =
                isStreamingBubble &&
                !m.content &&
                !m.reasoning &&
                !m.attachments?.length &&
                !m.uiSurfaces?.length &&
                !(m.activities?.length && displayPrefs.verbosity !== "compact");
              if (m.content && !hasTimelineText) {
                steps.push({
                  key: `reply-${m.id}`,
                  kind: "reply",
                  node: (
                    <>
                      <ChatMarkdown
                        content={m.content}
                        streaming={isStreamingBubble}
                        compact={displayPrefs.verbosity === "compact"}
                        plain={Boolean(m.error)}
                        caret={false}
                        mediaBaseDir={mediaBaseDir}
                      />
                      <MsgStreamLoader visible={isStreamingBubble} />
                    </>
                  ),
                });
              }

              const hasProcess = steps.some((step) => step.kind !== "reply");

              if (!m.content && isStreamingBubble && hasProcess) {
                steps.push({
                  key: `gen-${m.id}`,
                  kind: "generating",
                  active: true,
                  node: <MsgStreamLoader />,
                });
              } else if (!m.content && showLoaderAlone) {
                steps.push({
                  key: `gen-${m.id}`,
                  kind: "generating",
                  active: true,
                  node: <MsgStreamLoader alone />,
                });
              } else if (!m.content && isStreamingBubble && !hasProcess) {
                steps.push({
                  key: `gen-${m.id}`,
                  kind: "generating",
                  active: true,
                  node: <MsgStreamLoader />,
                });
              }

              if (steps.length === 0) return null;

              const groupedSteps = groupConsecutiveActivities(steps);

              if (!hasProcess) {
                return (
                  <>
                    {steps.map((step) => (
                      <div key={step.key}>{step.node}</div>
                    ))}
                  </>
                );
              }

              return (
                <MsgTimeline>
                  {groupedSteps.map((step, i) => {
                    const isLast = i === groupedSteps.length - 1;
                    if (isConsecutiveActivityGroup(step)) {
                      const activities = step.items.flatMap((item) =>
                        item.activity ? [item.activity] : [],
                      );
                      return (
                        <MsgTimelineStep
                          key={step.key}
                          kind={step.items[0]?.kind ?? "tool"}
                          grouped
                          active={activities.some(
                            (activity) => activity.status === "running",
                          )}
                          isLast={isLast}
                        >
                          <MsgActivityGroup
                            activities={activities}
                            defaultOpen={displayPrefs.processDefaultOpen}
                            forcedOpen={forcedProcessOpen}
                            showTimestamp={displayPrefs.showTimestamps}
                            mediaBaseDir={mediaBaseDir}
                            onOpenUrl={onOpenActivityUrl}
                          />
                        </MsgTimelineStep>
                      );
                    }
                    return (
                      <MsgTimelineStep
                        key={step.key}
                        kind={step.kind}
                        activity={step.activity}
                        active={step.active}
                        isLast={isLast}
                      >
                        {step.node}
                      </MsgTimelineStep>
                    );
                  })}
                </MsgTimeline>
              );
            })()
          )}
          {displayPrefs.showTimestamps && m.createdAt ? (
            <div className="msg-timestamp">
              {new Date(m.createdAt).toLocaleTimeString()}
            </div>
          ) : null}
          {m.role === "assistant" &&
          !isStreamingBubble &&
          m.id !== "welcome" ? (
            <div className="assistant-message-footer">
              <MessageActions
                messageId={m.id}
                content={m.content}
                role="assistant"
                disabled={streaming || turnInFlight}
                onBranch={onBranchMessage}
                onOpenMenu={(anchor) => {
                  const rect = anchor.getBoundingClientRect();
                  openAssistantTurnMenu(
                    m.id,
                    rect.right,
                    rect.bottom + 4,
                    false,
                    anchor,
                  );
                }}
              />
              {m.usage || m.generationDurationSec ? (
                <MessageTokenStats
                  usage={m.usage}
                  tokensPerSec={m.tokensPerSec}
                  generationDurationSec={m.generationDurationSec}
                />
              ) : null}
            </div>
          ) : null}
          {m.role === "assistant" &&
          !isStreamingBubble &&
          m.id !== "welcome" ? (
            <TurnChangeSummaryCard
              message={m}
              projectId={projectId}
              onReview={onOpenFileReview}
            />
          ) : null}
        </div>
        {!isStreamingBubble &&
        m.id !== "welcome" &&
        m.role === "user" &&
        canEditUserMessage &&
        !isEditingUserMessage ? (
          <MessageActions
            messageId={m.id}
            content={m.content}
            role="user"
            disabled={streaming}
            onEdit={() => beginUserMessageEdit(m)}
          />
        ) : null}
      </div>
    </div>
  );
}

export default memo(ChatMessageRowImpl);
