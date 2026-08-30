import { useCallback, useRef, useState } from "react";
import { motion, useReducedMotion } from "framer-motion";
import { MessageSquare, X } from "lucide-react";
import { useChatSession } from "../../hooks/chat/useChatSession";
import type { ChatDisplayPrefs } from "../../hooks/chat/useChatDisplayPrefs";
import { useTransientToast } from "../../hooks/ui/useTransientToast";
import { useI18n } from "../../i18n/LocaleContext";
import type { ChatWorkMode } from "../../lib/chat/chatMode";
import type { FileChangeItem } from "../../lib/chat/taskProgress";
import { usagePercent } from "../../lib/chat/contextUsage";
import type {
  ChatThinkingPrefs,
  ThinkingLevel,
} from "../../lib/chat/thinkingPrefs";
import type {
  ModelCapabilities,
  ModelPricingMeta,
  ModelReasoningMeta,
  ProviderDto,
} from "../../types";
import ChatView from "./ChatView";

type Props = {
  sessionId: string;
  parentSessionId: string | null;
  activeProjectId: string;
  provider: ProviderDto;
  providers: ProviderDto[];
  displayPrefs: ChatDisplayPrefs;
  interactionMode: ChatWorkMode;
  thinkingPrefs: ChatThinkingPrefs;
  showThinkingControls: boolean;
  reasoningMeta: ModelReasoningMeta | null;
  modelCapabilities: ModelCapabilities | null;
  modelPricing: ModelPricingMeta | null;
  contextWindow: number;
  onThinkingLevelChange: (level: ThinkingLevel) => void;
  onToggleThinking: () => void;
  onOpenMcpSettings: () => void;
  onOpenContext: () => void;
  onOpenFileReview: (file: FileChangeItem, files: FileChangeItem[]) => void;
  onClose: () => void | Promise<void>;
};

/**
 * Side Chat：复用完整 ChatView 与 useChatSession 能力，
 * 但关闭浏览器端快照持久化，由父级在关闭时删除 ephemeral backend session。
 */
export default function SideChatPanel({
  sessionId,
  parentSessionId,
  activeProjectId,
  provider,
  providers,
  displayPrefs,
  interactionMode,
  thinkingPrefs,
  showThinkingControls,
  reasoningMeta,
  modelCapabilities,
  modelPricing,
  contextWindow,
  onThinkingLevelChange,
  onToggleThinking,
  onOpenMcpSettings,
  onOpenContext,
  onOpenFileReview,
  onClose,
}: Props) {
  const { t } = useI18n();
  const reducedMotion = useReducedMotion();
  const { showToast, toastHost } = useTransientToast();
  const displayPrefsRef = useRef(displayPrefs);
  displayPrefsRef.current = displayPrefs;
  const [sideMode, setSideMode] = useState<ChatWorkMode>(interactionMode);

  const chat = useChatSession({
    activeProjectId,
    activeProvider: provider,
    providers,
    chatMode: sideMode,
    onChatModeChange: setSideMode,
    chatDisplayPrefsRef: displayPrefsRef,
    t,
    showTransientToast: showToast,
    persistClientState: false,
    initialSessionId: sessionId,
    initialParentSessionId: parentSessionId,
    initialEphemeral: true,
  });

  const close = useCallback(async () => {
    if (chat.streaming || chat.turnInFlight) {
      await chat.stopStream();
    }
    await onClose();
  }, [chat.stopStream, chat.streaming, chat.turnInFlight, onClose]);

  return (
    <motion.aside
      className="side-chat-panel"
      aria-label={t("chat.side.panel")}
      initial={
        reducedMotion ? { opacity: 0 } : { opacity: 0, x: 12, scale: 0.98 }
      }
      animate={{ opacity: 1, x: 0, scale: 1 }}
      exit={
        reducedMotion
          ? { opacity: 0, transition: { duration: 0.12 } }
          : {
              opacity: 0,
              x: 12,
              scale: 0.985,
              transition: { duration: 0.16, ease: "easeOut" },
            }
      }
      transition={{
        duration: reducedMotion ? 0.12 : 0.24,
        ease: [0.22, 1, 0.36, 1],
      }}
    >
      <header className="side-chat-head">
        <span className="side-chat-mark" aria-hidden>
          <MessageSquare size={15} />
        </span>
        <div className="side-chat-title">
          <strong>{t("chat.side.title")}</strong>
          <span title={t("chat.side.subtitle")}>{t("chat.side.subtitle")}</span>
        </div>
        <button
          type="button"
          className="side-chat-close"
          onClick={() => void close()}
          title={t("chat.side.close")}
          aria-label={t("chat.side.close")}
        >
          <X size={15} aria-hidden />
        </button>
      </header>

      <div className="side-chat-body">
        <ChatView
          sessionId={chat.sessionId}
          messages={chat.messages}
          input={chat.input}
          attachments={chat.attachments}
          streaming={chat.streaming}
          turnInFlight={chat.turnInFlight}
          completionCelebrationId={chat.completionCelebrationId}
          streamPaused={chat.streamPaused}
          sendBlocked={chat.isCompacting || chat.sessionReadOnly}
          sendBlockedReason={
            chat.isCompacting
              ? t("chat.compactInProgress")
              : chat.sessionReadOnly
                ? t("chat.sessionEndedReadOnly")
                : undefined
          }
          displayPrefs={displayPrefs}
          emptyMode={chat.emptyMode}
          focusMessageId={chat.focusMessageId}
          onFocusConsumed={() => chat.setFocusMessageId(null)}
          onInputChange={chat.setInput}
          onAttachmentsChange={chat.setAttachments}
          onSend={chat.send}
          queuedFollowUps={chat.queuedFollowUps}
          onRemoveQueuedFollowUp={chat.removeQueuedFollowUp}
          onUpdateQueuedFollowUpText={chat.updateQueuedFollowUpText}
          onMoveQueuedFollowUp={chat.moveQueuedFollowUp}
          onSteerQueuedFollowUp={chat.steerQueuedFollowUp}
          onCloseQueuedFollowUps={chat.closeQueuedFollowUps}
          modeSwitchPrompt={chat.modeSwitchPrompt}
          onApproveModeSwitch={chat.approveModeSwitch}
          onDismissModeSwitch={chat.dismissModeSwitch}
          parallelTasks={chat.parallelTasks}
          onCancelParallelTask={chat.cancelParallelTask}
          onWriteParallelSummary={chat.writeParallelSummary}
          onClearSettledParallel={chat.clearSettledParallel}
          pendingInterrupts={chat.sessionPendingInterrupts}
          onUiAction={chat.onUiAction}
          onPauseStream={chat.pauseStream}
          onResumeStream={chat.resumeStream}
          onStopStream={chat.stopStream}
          onNewChat={() => void close()}
          onPickWelcomePrompt={chat.setInput}
          showThinkingControls={showThinkingControls}
          reasoningMeta={reasoningMeta}
          thinkingPrefs={thinkingPrefs}
          onToggleThinking={onToggleThinking}
          onThinkingLevelChange={onThinkingLevelChange}
          onOpenMcpSettings={onOpenMcpSettings}
          chatMode={sideMode}
          onChatModeChange={setSideMode}
          onOpenContext={onOpenContext}
          onOpenFileReview={onOpenFileReview}
          onRegenerateMessage={chat.regenerateMessage}
          onEditUserMessage={chat.editUserMessage}
          contextUsage={chat.contextUsage}
          contextWindow={contextWindow}
          modelId={provider.model}
          cronProviders={providers.map((item) => ({
            id: item.id,
            name: item.display_name,
            model: item.model,
            kind: item.kind,
          }))}
          cronActiveProviderId={provider.id}
          modelCapabilities={modelCapabilities}
          modelPricing={modelPricing}
          contextUsagePercent={
            chat.contextUsage && contextWindow > 0
              ? usagePercent(chat.contextUsage.totalTokens, contextWindow)
              : null
          }
        />
      </div>
      {toastHost}
    </motion.aside>
  );
}
