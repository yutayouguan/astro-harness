import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type Dispatch,
  type RefObject,
  type SetStateAction,
} from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { sealOpenReasoning } from "../../lib/chat/chatTimeline";
import { elapsedSecSince } from "../../lib/chat/elapsedSec";
import {
  buildElicitationContent,
  elicitationRequestId,
  resolveElicitationAction,
} from "../../lib/chat/elicitation";
import {
  type ChatInteractionMode,
  type ChatWorkMode,
  type ModeSwitchRequest,
  shouldAutoApproveModeSwitch,
} from "../../lib/chat/chatMode";
import {
  MAX_QUEUED_FOLLOWUPS,
  newQueuedFollowUpId,
  type QueuedFollowUp,
} from "../../lib/chat/followUpQueue";
import {
  buildParallelTasksSummaryMarkdown,
  countRunningParallel,
} from "../../lib/chat/parallelTasks";
import {
  clearChatSession,
  isChatCleared,
  isWelcomeOnly,
  loadChatSession,
  loadContextUsageForSession,
  saveChatSession,
  saveContextUsageForSession,
  saveEphemeralSessionMeta,
} from "../../lib/chat/chatSessionStore";
import {
  projectResponseItemsToEntries,
  settleRestoredActivities,
} from "../../lib/chat/projectResponseItemsToEntries";
import { findLastUserEntryIndex } from "../../lib/chat/turnEditing";
import { dispatchSessionsChanged } from "../../lib/chat/sessionManagement";
import type {
  ArtifactDto,
  ChatAttachment,
  ChatAttachmentKind,
  ChatEmptyMode,
  ResponseItemHistoryDto,
  ConversationEntry,
  TurnTokenUsage,
  PendingInterrupt,
  ProviderDto,
} from "../../types";
import type { ContextUsageSnapshot } from "../../lib/chat/contextUsage";
import type { ChatDisplayPrefs } from "./useChatDisplayPrefs";
import type { ShowToastOptions } from "../ui/useTransientToast";
import type { MessageKey } from "../../i18n/messages";
import type { ChatRightTab } from "../../components/chat/ChatRightPanel";
import { useChatStreamBuffers } from "./useChatStreamBuffers";
import { useGeneratingPreview } from "./useGeneratingPreview";
import { useBrowserPreview } from "./useBrowserPreview";
import { useParallelTasks } from "./useParallelTasks";
import { useSend, type SendOpts } from "./useSend";
import { useConfirm } from "../ui/DialogContext";

type TFn = (key: MessageKey, vars?: Record<string, string>) => string;
type ShowToastFn = (msg: string, opts?: ShowToastOptions) => void;
type StatusPhase = "ready" | "connecting" | "generating" | "error";
type NavId = "chat" | "cron" | "loop" | "skills" | "settings";

const MAX_ATTACHMENTS = 8;
const MAX_INLINE_BYTES = 4 * 1024 * 1024;

export { projectResponseItemsToEntries } from "../../lib/chat/projectResponseItemsToEntries";

function countChatBubbles(msgs: ConversationEntry[]): number {
  return msgs.filter(
    (m) => m.id !== "welcome" && (m.role === "user" || m.role === "assistant"),
  ).length;
}

function kindFromMime(mime: string, name: string): ChatAttachmentKind {
  if (mime.startsWith("image/")) return "image";
  if (mime.startsWith("video/")) return "video";
  if (mime.startsWith("audio/")) return "audio";
  const ext = name.split(".").pop()?.toLowerCase() ?? "";
  if (["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "heic"].includes(ext))
    return "image";
  if (["mp4", "webm", "mov", "mkv", "avi"].includes(ext)) return "video";
  if (["mp3", "wav", "m4a", "aac", "ogg", "flac"].includes(ext)) return "audio";
  return "file";
}

function calcTokensPerSec(
  completionTokens: number,
  durationMs: number,
): number | undefined {
  if (completionTokens <= 0 || durationMs <= 0) return undefined;
  const sec = Math.max(0.1, durationMs / 1000);
  return Math.round((completionTokens / sec) * 10) / 10;
}

export interface UseChatSessionDeps {
  activeProjectId: string;
  activeProvider: ProviderDto | undefined;
  providers: ProviderDto[];
  chatMode: ChatInteractionMode;
  onChatModeChange: (mode: ChatWorkMode) => void;
  chatDisplayPrefsRef: RefObject<ChatDisplayPrefs>;
  t: TFn;
  showTransientToast: ShowToastFn;
  nav?: NavId;
  setNav?: Dispatch<SetStateAction<NavId>>;
  /** 是否把当前 UI 会话快照写入 localStorage。 */
  persistClientState?: boolean;
  /** 独立聊天表面可直接绑定已创建的 backend session。 */
  initialSessionId?: string | null;
  initialParentSessionId?: string | null;
  initialExcludedTurnCount?: number;
  initialEphemeral?: boolean;
}

export function useChatSession({
  activeProjectId,
  activeProvider,
  providers,
  chatMode,
  onChatModeChange,
  chatDisplayPrefsRef,
  t,
  showTransientToast,
  nav = "chat",
  setNav = () => {},
  persistClientState = true,
  initialSessionId = null,
  initialParentSessionId = null,
  initialExcludedTurnCount = 0,
  initialEphemeral = false,
}: UseChatSessionDeps) {
  const [initialStored] = useState(() => {
    const stored = persistClientState ? loadChatSession() : null;
    if (!stored) return null;
    return {
      ...stored,
      messages: settleRestoredActivities(stored.messages),
    };
  });
  // ── Core state ────────────────────────────────────────────────────────────
  const [messages, setMessages] = useState<ConversationEntry[]>(() => {
    if (initialStored?.messages?.length) return initialStored.messages;
    return [];
  });
  const [emptyMode, setEmptyMode] = useState<ChatEmptyMode>(() => {
    if (initialSessionId) return null;
    return initialStored && !isWelcomeOnly(initialStored.messages)
      ? null
      : "chat";
  });
  const [input, setInput] = useState("");
  const [attachments, setAttachments] = useState<ChatAttachment[]>([]);
  const [queuedFollowUps, setQueuedFollowUps] = useState<QueuedFollowUp[]>([]);
  const [queueKick, setQueueKick] = useState(0);
  const [modeSwitchPrompt, setModeSwitchPrompt] =
    useState<ModeSwitchRequest | null>(null);
  const modeSwitchArmedRef = useRef(false);
  const [streaming, setStreaming] = useState(false);
  const [turnInFlight, setTurnInFlight] = useState(false);
  const [completionCelebrationId, setCompletionCelebrationId] = useState(0);
  const turnInFlightRef = useRef(false);
  const sendStartLockRef = useRef(false);
  const lastStreamActivityAtRef = useRef(0);
  const sessionWorktreeRef = useRef<{
    sessionId: string;
    id: string;
    path: string;
    branch?: string | null;
    headSha: string;
  } | null>(null);
  const steeringQueueIdsRef = useRef(new Set<string>());
  const stopStreamRef = useRef<() => Promise<void>>(() => Promise.resolve());
  const checkpointFiredForTurnRef = useRef(false);
  const [streamPaused, setStreamPaused] = useState(false);
  const [tokenUsage, setTokenUsage] = useState<TurnTokenUsage | null>(null);
  const [contextUsage, setContextUsage] = useState<ContextUsageSnapshot | null>(
    () => initialStored?.contextUsage ?? null,
  );
  const [sessionId, setSessionId] = useState<string | null>(
    () => initialSessionId ?? initialStored?.sessionId ?? null,
  );
  const [sideParentSessionId, setSideParentSessionId] = useState<string | null>(
    () => initialParentSessionId ?? initialStored?.parentSessionId ?? null,
  );
  const [sideExcludedTurnCount, setSideExcludedTurnCount] = useState(
    () => initialExcludedTurnCount || initialStored?.excludedTurnCount || 0,
  );
  const [sessionEphemeral, setSessionEphemeral] = useState(
    () => initialEphemeral || initialStored?.ephemeral || false,
  );
  const [sessionPendingInterrupts, setSessionPendingInterrupts] = useState<
    PendingInterrupt[]
  >(() => initialStored?.pendingInterrupts ?? []);
  const [sessionReadOnly, setSessionReadOnly] = useState(false);
  const [sessionEndReason, setSessionEndReason] = useState<string | null>(null);
  const [currentTurnId, setCurrentTurnId] = useState<string | null>(null);
  const [focusMessageId, setFocusMessageId] = useState<string | null>(null);
  const [isCompacting, setIsCompacting] = useState(false);
  const [status, setStatus] = useState<"ready" | "busy" | "error">("ready");
  const [statusPhase, setStatusPhase] = useState<StatusPhase>("ready");
  const [statusDetail, setStatusDetail] = useState<string | null>(null);
  const [memoryPendingCount, setMemoryPendingCount] = useState(0);
  const [chatRightOpen, setChatRightOpen] = useState(() => {
    if (!persistClientState) return false;
    try {
      return localStorage.getItem("astro.chatRightOpen") === "1";
    } catch {
      return false;
    }
  });
  const [chatRightTab, setChatRightTab] = useState<ChatRightTab>("summary");
  const confirm = useConfirm();

  useEffect(() => {
    if (
      !persistClientState ||
      !sessionEphemeral ||
      !sessionId ||
      isWelcomeOnly(messages)
    )
      return;
    saveEphemeralSessionMeta(
      sessionId,
      sideParentSessionId,
      sideExcludedTurnCount,
    );
  }, [
    messages,
    sessionEphemeral,
    sessionId,
    sideParentSessionId,
    sideExcludedTurnCount,
    persistClientState,
  ]);

  // ── 生成中文件实时预览 ──────────────────────────────────────────────────────
  const { preview: generatingPreview, api: generatingPreviewApi } =
    useGeneratingPreview({});
  const {
    preview: browserPreview,
    api: browserPreviewApi,
    control: controlBrowser,
    applyResult: applyBrowserResult,
    dismiss: dismissBrowserPreview,
  } = useBrowserPreview(sessionId);

  // ── Refs ──────────────────────────────────────────────────────────────────
  const unlistenRef = useRef<(() => void) | null>(null);
  const restoringRef = useRef(false);
  const pendingKeepChatBubblesRef = useRef<number | null>(null);
  const compactingRef = useRef(false);
  const lastRecommendCompactToastAtRef = useRef(0);
  const memoryToastDedupeRef = useRef<{ key: string; at: number } | null>(null);

  // ── Stream buffer layer ───────────────────────────────────────────────────
  const {
    enqueueStreamToken,
    enqueueStreamReasoning,
    enqueueToolDelta,
    flushStreamTokens,
    flushToolDeltas,
    clearStreamBuffers,
    settleMessageUsage,
    streamGenRef,
    currentRunIdRef,
    activeAssistantIdRef,
    streamStartRef,
    firstTokenRef,
    pendingUsageRef,
    streamPendingRef,
    toolDeltaIdsRef,
    toolDeltaRafRef,
    streamRafRef,
  } = useChatStreamBuffers(setMessages);

  const celebrateTaskCompletion = useCallback(() => {
    setCompletionCelebrationId((current) => current + 1);
  }, []);

  const {
    parallelTasks,
    startParallelTask,
    cancelParallelTask,
    resumeParallelHitl,
    clearSettledParallel,
  } = useParallelTasks({
    activeProvider,
    setMessages,
    setEmptyMode,
    setInput,
    setAttachments,
    showTransientToast,
    t,
    onTaskSucceeded: celebrateTaskCompletion,
  });

  // ── Send ──────────────────────────────────────────────────────────────────
  const onModeSwitchDetected = useCallback((_req: ModeSwitchRequest) => {
    modeSwitchArmedRef.current = true;
  }, []);
  const onModeSwitchPrompt = useCallback((req: ModeSwitchRequest) => {
    if (!modeSwitchArmedRef.current) return;
    modeSwitchArmedRef.current = false;
    setModeSwitchPrompt(req);
  }, []);
  const onUserInputCommitted = useCallback(
    (clientMessageId: string) => {
      if (!steeringQueueIdsRef.current.delete(clientMessageId)) return;
      setQueuedFollowUps((prev) => {
        const hit = prev.find((item) => item.id === clientMessageId);
        if (!hit) return prev;
        for (const attachment of hit.attachments) {
          if (attachment.previewUrl) URL.revokeObjectURL(attachment.previewUrl);
        }
        return prev.filter((item) => item.id !== clientMessageId);
      });
      setQueueKick((value) => value + 1);
      showTransientToast(t("chat.queue.steerSent"), { tone: "success" });
    },
    [showTransientToast, t],
  );

  const { send: sendImmediate } = useSend({
    projectId: activeProjectId,
    composer: {
      input,
      attachments,
      setInput,
      setAttachments,
    },
    execution: {
      streaming,
      isCompacting,
      sessionReadOnly,
      sessionEndReason,
      turnInFlightRef,
      sendStartLockRef,
      compactingRef,
      setStreaming,
      setStreamPaused,
      setTurnInFlight,
    },
    model: {
      activeProvider,
      providers,
      chatMode,
    },
    session: {
      sessionId,
      emptyMode,
      pendingInterrupts: sessionPendingInterrupts,
      setSessionId,
      setEmptyMode,
      setPendingInterrupts: setSessionPendingInterrupts,
      setCurrentTurnId,
    },
    stream: {
      clear: clearStreamBuffers,
      enqueueToken: enqueueStreamToken,
      enqueueReasoning: enqueueStreamReasoning,
      enqueueToolDelta,
      flushTokens: flushStreamTokens,
      flushToolDeltas,
      settleUsage: settleMessageUsage,
      generatingPreview: generatingPreviewApi,
      browserPreview: browserPreviewApi,
    },
    refs: {
      generation: streamGenRef,
      currentRunId: currentRunIdRef,
      activeAssistantId: activeAssistantIdRef,
      streamStart: streamStartRef,
      firstToken: firstTokenRef,
      pendingUsage: pendingUsageRef,
      pendingText: streamPendingRef,
      toolDeltaIds: toolDeltaIdsRef,
      toolDeltaFrame: toolDeltaRafRef,
      streamFrame: streamRafRef,
      unlisten: unlistenRef,
      pendingKeepChatBubbles: pendingKeepChatBubblesRef,
      lastRecommendCompactToastAt: lastRecommendCompactToastAtRef,
      lastStreamActivityAt: lastStreamActivityAtRef,
    },
    view: {
      setMessages,
      setTokenUsage,
      setContextUsage,
      setStatus,
      setStatusPhase,
      setStatusDetail,
    },
    callbacks: {
      showToast: showTransientToast,
      onModeSwitchDetected,
      onModeSwitchPrompt,
      onUserInputCommitted,
      onTurnSucceeded: celebrateTaskCompletion,
    },
    t,
    chatDisplayPrefsRef,
    persistContextUsage: persistClientState,
  });

  const prevChatModeRef = useRef(chatMode);
  useEffect(() => {
    const prev = prevChatModeRef.current;
    if (prev === chatMode) return;
    prevChatModeRef.current = chatMode;

    if (prev === "agent" && chatMode !== "agent") {
      const wt = sessionWorktreeRef.current;
      if (wt) {
        void invoke("cleanup_task_worktree", {
          worktreeId: wt.id,
        }).catch(() => {});
        sessionWorktreeRef.current = null;
      }
    }
  }, [chatMode]);

  const onChatModeChangeRef = useRef(onChatModeChange);
  onChatModeChangeRef.current = onChatModeChange;
  const sendImmediateRef = useRef(sendImmediate);
  sendImmediateRef.current = sendImmediate;

  const queueDrainLockRef = useRef(false);
  const queueFailedIdRef = useRef<string | null>(null);
  const modeSwitchPromptRef = useRef(modeSwitchPrompt);
  modeSwitchPromptRef.current = modeSwitchPrompt;

  useEffect(() => {
    if (turnInFlight) return;
    steeringQueueIdsRef.current.clear();
    setQueuedFollowUps((prev) => {
      if (!prev.some((item) => item.delivery === "steering")) return prev;
      return prev.map((item) =>
        item.delivery === "steering" ? { ...item, delivery: "queued" } : item,
      );
    });
  }, [turnInFlight]);

  const dismissModeSwitch = useCallback(() => {
    const req = modeSwitchPromptRef.current;
    modeSwitchPromptRef.current = null;
    setModeSwitchPrompt(null);
    modeSwitchArmedRef.current = false;
    if (!req) {
      setQueueKick((k) => k + 1);
      return;
    }
    const inject =
      `[Plan review: continue planning]\n` +
      `The user is not authorizing execution yet. ` +
      `Stay in Plan mode and revise or clarify the plan. ` +
      `Do not call switch_mode again until the plan has materially changed or the user explicitly asks.`;
    showTransientToast(t("chat.modeSwitch.declined"), { tone: "warning" });
    void (async () => {
      await sendImmediateRef.current({ text: inject });
      setQueueKick((k) => k + 1);
    })();
  }, [showTransientToast, t]);

  const writeParallelSummary = useCallback(() => {
    if (parallelTasks.length === 0) return;
    if (countRunningParallel(parallelTasks) > 0) {
      showTransientToast(t("chat.task.summaryStillRunning"), {
        tone: "warning",
      });
      return;
    }
    const replyByAssistantId = new Map<string, string>();
    for (const m of messages) {
      if (m.role === "assistant") {
        replyByAssistantId.set(m.id, m.content ?? "");
      }
    }
    const content = buildParallelTasksSummaryMarkdown(
      parallelTasks,
      replyByAssistantId,
    );
    const id = `summary-${Date.now()}`;
    setMessages((prev) => [
      ...prev,
      {
        id,
        role: "assistant",
        content,
        createdAt: Date.now(),
      },
    ]);
    setEmptyMode(null);
    setFocusMessageId(id);
    showTransientToast(t("chat.task.summaryWritten"), { tone: "success" });
  }, [parallelTasks, messages, showTransientToast, t]);

  const approveModeSwitch = useCallback(async () => {
    const req = modeSwitchPromptRef.current;
    if (!req) {
      setModeSwitchPrompt(null);
      setQueueKick((k) => k + 1);
      return;
    }
    modeSwitchPromptRef.current = null;
    setModeSwitchPrompt(null);
    modeSwitchArmedRef.current = false;
    onChatModeChangeRef.current(req.to);
    if (req.to === "agent" && req.summary) {
      const inject = `[Authorized mode switch: Plan → Agent]\n\nConfirmed plan:\n${req.summary}`;
      await sendImmediateRef.current({
        text: inject,
        interactionMode: "agent",
      });
    } else if (req.to === "plan") {
      const inject =
        `[Authorized mode switch: Agent → Plan]\n` +
        `Reason: ${req.reason}\n` +
        `Stay in Plan mode: produce a clear step-by-step plan. Do not write files or run side-effect tools until switched back to Agent.`;
      await sendImmediateRef.current({
        text: inject,
        interactionMode: "plan",
      });
    }
    setQueueKick((k) => k + 1);
  }, []);

  useEffect(() => {
    if (!shouldAutoApproveModeSwitch(modeSwitchPrompt)) return;
    // 进入只读 Plan 是收窄权限，自动接受；返回 Agent 始终留给用户显式审阅。
    void approveModeSwitch();
  }, [approveModeSwitch, modeSwitchPrompt]);

  /** 当前任务忙时根据 sendMode 投递；空闲时立即发送。 */
  const send = useCallback(
    async (opts?: SendOpts) => {
      // 同一次点击尚在解析 Skill/MCP 时，忽略再次提交；真正进入 turn 后才允许入队。
      if (sendStartLockRef.current) return;
      if (
        streaming ||
        turnInFlightRef.current ||
        sessionPendingInterrupts.length > 0
      ) {
        const text = (opts?.text ?? input).trim();
        const pending = opts?.attachments ?? attachments;
        if (!text && pending.length === 0) return;
        if (sessionReadOnly || isCompacting) {
          showTransientToast(
            isCompacting
              ? t("chat.compactInProgress")
              : sessionEndReason === "compacted" || !sessionEndReason
                ? t("chat.sessionCompactedReadOnly")
                : t("chat.sessionEndedReadOnly"),
            { tone: "warning" },
          );
          return;
        }
        const mode = opts?.sendMode ?? "queue";

        if (mode === "steer") {
          const turnId = currentTurnId;
          if (!sessionId || !turnId || !turnInFlightRef.current) {
            showTransientToast(t("chat.queue.steerUnavailable"), {
              tone: "warning",
            });
            return;
          }
          const clientId = newQueuedFollowUpId();
          try {
            const accepted = await invoke<boolean>("steer_chat", {
              sessionId,
              expectedTurnId: turnId,
              clientMessageId: clientId,
              content: text,
              attachments: (pending ?? []).map((a) => ({
                name: a.name,
                mime: a.mime,
                kind: a.kind,
                size: a.size,
                dataBase64: a.dataBase64 ?? null,
                localPath: a.localPath ?? null,
              })),
            });
            if (!accepted) {
              showTransientToast(t("chat.queue.steerUnavailable"), {
                tone: "warning",
              });
              return;
            }
            showTransientToast(t("chat.queue.steerSent"), { tone: "info" });
          } catch (error) {
            showTransientToast(
              t("chat.queue.steerFailed", { error: String(error) }),
              { tone: "error" },
            );
            return;
          }
          if (opts?.text == null) setInput("");
          if (opts?.attachments == null) setAttachments([]);
          return;
        }

        if (mode === "interrupt") {
          await stopStreamRef.current();
          await sendImmediate(opts);
          return;
        }

        // 排队模式：加入 follow-up 队列，Agent 完成后自动出队
        let overflow = false;
        setQueuedFollowUps((prev) => {
          if (prev.length >= MAX_QUEUED_FOLLOWUPS) {
            overflow = true;
            return prev;
          }
          return [
            ...prev,
            {
              id: newQueuedFollowUpId(),
              text,
              attachments: pending.map((a) => ({ ...a })),
              createdAt: Date.now(),
            },
          ];
        });
        if (overflow) {
          showTransientToast(
            t("chat.queue.full", { max: String(MAX_QUEUED_FOLLOWUPS) }),
            {
              tone: "warning",
            },
          );
          return;
        }
        if (opts?.text == null) setInput("");
        if (opts?.attachments == null) setAttachments([]);
        return;
      }
      await sendImmediate(opts);
    },
    [
      streaming,
      turnInFlight,
      input,
      attachments,
      sessionPendingInterrupts.length,
      sessionReadOnly,
      isCompacting,
      sessionEndReason,
      sessionId,
      currentTurnId,
      sendImmediate,
      showTransientToast,
      t,
    ],
  );

  useEffect(() => {
    if (streaming || turnInFlight || isCompacting || sessionReadOnly) return;
    if (sessionPendingInterrupts.length > 0) return;
    if (modeSwitchPrompt) return;
    if (queueDrainLockRef.current) return;

    const head = queuedFollowUps[0];
    if (!head) return;
    if (head.delivery === "steering") return;
    if (queueFailedIdRef.current === head.id) return;

    queueDrainLockRef.current = true;
    setQueuedFollowUps((prev) =>
      prev[0]?.id === head.id ? prev.slice(1) : prev,
    );

    void (async () => {
      try {
        const started = await sendImmediate({
          text: head.text,
          attachments: head.attachments,
        });
        if (!started) {
          queueFailedIdRef.current = head.id;
          setQueuedFollowUps((prev) => [head, ...prev]);
          showTransientToast(t("chat.queue.drainFailed"), { tone: "warning" });
        } else {
          queueFailedIdRef.current = null;
        }
      } catch {
        queueFailedIdRef.current = head.id;
        setQueuedFollowUps((prev) => [head, ...prev]);
      } finally {
        queueDrainLockRef.current = false;
      }
    })();
  }, [
    streaming,
    turnInFlight,
    isCompacting,
    sessionReadOnly,
    sessionPendingInterrupts.length,
    modeSwitchPrompt,
    queuedFollowUps,
    queueKick,
    sendImmediate,
    showTransientToast,
    t,
  ]);

  const dropQueuedFollowUp = useCallback(
    (id: string, revokePreview: boolean) => {
      if (queueFailedIdRef.current === id) queueFailedIdRef.current = null;
      setQueuedFollowUps((prev) => {
        const hit = prev.find((q) => q.id === id);
        if (hit && revokePreview) {
          for (const a of hit.attachments) {
            if (a.previewUrl) URL.revokeObjectURL(a.previewUrl);
          }
        }
        return prev.filter((q) => q.id !== id);
      });
      setQueueKick((k) => k + 1);
    },
    [],
  );

  const removeQueuedFollowUp = useCallback(
    (id: string) => {
      if (steeringQueueIdsRef.current.has(id)) return;
      dropQueuedFollowUp(id, true);
    },
    [dropQueuedFollowUp],
  );

  const updateQueuedFollowUpText = useCallback((id: string, text: string) => {
    if (steeringQueueIdsRef.current.has(id)) return;
    if (queueFailedIdRef.current === id) queueFailedIdRef.current = null;
    setQueuedFollowUps((prev) =>
      prev.map((q) => (q.id === id ? { ...q, text } : q)),
    );
    setQueueKick((k) => k + 1);
  }, []);

  const moveQueuedFollowUp = useCallback((id: string, dir: -1 | 1) => {
    if (steeringQueueIdsRef.current.has(id)) return;
    if (queueFailedIdRef.current === id) queueFailedIdRef.current = null;
    setQueuedFollowUps((prev) => {
      const i = prev.findIndex((q) => q.id === id);
      if (i < 0) return prev;
      const j = i + dir;
      if (j < 0 || j >= prev.length) return prev;
      const next = [...prev];
      const tmp = next[i]!;
      next[i] = next[j]!;
      next[j] = tmp;
      return next;
    });
    setQueueKick((k) => k + 1);
  }, []);

  const steerQueuedFollowUp = useCallback(
    async (id: string) => {
      const item = queuedFollowUps.find((queued) => queued.id === id);
      if (
        !item ||
        !sessionId ||
        !currentTurnId ||
        !turnInFlightRef.current ||
        steeringQueueIdsRef.current.has(id)
      )
        return false;
      steeringQueueIdsRef.current.add(id);
      try {
        const accepted = await invoke<boolean>("steer_chat", {
          sessionId,
          expectedTurnId: currentTurnId,
          clientMessageId: id,
          content: item.text,
          attachments: item.attachments.map((a) => ({
            name: a.name,
            mime: a.mime,
            kind: a.kind,
            size: a.size,
            dataBase64: a.dataBase64 ?? null,
            localPath: a.localPath ?? null,
          })),
        });
        if (!accepted) {
          steeringQueueIdsRef.current.delete(id);
          showTransientToast(t("chat.queue.steerUnavailable"), {
            tone: "warning",
          });
          return false;
        }
        setQueuedFollowUps((prev) =>
          prev.map((queued) =>
            queued.id === id ? { ...queued, delivery: "steering" } : queued,
          ),
        );
        return true;
      } catch (error) {
        steeringQueueIdsRef.current.delete(id);
        showTransientToast(
          t("chat.queue.steerFailed", { error: String(error) }),
          {
            tone: "error",
          },
        );
        return false;
      }
    },
    [
      currentTurnId,
      queuedFollowUps,
      sessionId,
      showTransientToast,
      t,
      turnInFlightRef,
    ],
  );

  const openQueuedFollowUpInNewTask = useCallback(
    async (id: string) => {
      const item = queuedFollowUps.find((queued) => queued.id === id);
      if (!item) return false;
      const started = await startParallelTask({
        text: item.text,
        attachments: item.attachments,
        clearComposer: false,
      });
      if (!started) return false;
      // 附件预览 URL 已转移给独立任务气泡，不能在这里 revoke。
      dropQueuedFollowUp(id, false);
      showTransientToast(t("chat.queue.openedInNewTask"), { tone: "success" });
      return true;
    },
    [
      dropQueuedFollowUp,
      queuedFollowUps,
      showTransientToast,
      startParallelTask,
      t,
    ],
  );

  const closeQueuedFollowUps = useCallback(() => {
    if (queuedFollowUps.length === 0) return true;
    if (steeringQueueIdsRef.current.size > 0) {
      showTransientToast(t("chat.queue.steerPending"), { tone: "warning" });
      return false;
    }
    if (input.trim() || attachments.length > 0) {
      showTransientToast(t("chat.queue.closeNeedsEmptyComposer"), {
        tone: "warning",
      });
      return false;
    }
    const restoredAttachments = queuedFollowUps.flatMap(
      (item) => item.attachments,
    );
    if (restoredAttachments.length > MAX_ATTACHMENTS) {
      showTransientToast(
        t("chat.queue.closeTooManyAttachments", {
          max: String(MAX_ATTACHMENTS),
        }),
        { tone: "warning" },
      );
      return false;
    }
    const restoredText = queuedFollowUps
      .map((item) => item.text.trim())
      .filter(Boolean)
      .join("\n\n");
    setInput(restoredText);
    setAttachments(restoredAttachments);
    setQueuedFollowUps([]);
    queueFailedIdRef.current = null;
    setQueueKick((k) => k + 1);
    showTransientToast(t("chat.queue.closed"), { tone: "success" });
    return true;
  }, [attachments.length, input, queuedFollowUps, showTransientToast, t]);

  // ── Persist session ───────────────────────────────────────────────────────
  useEffect(() => {
    if (!persistClientState || restoringRef.current || streaming) return;
    saveChatSession(
      sessionId,
      messages,
      sessionPendingInterrupts,
      contextUsage,
    );
    if (sessionId && contextUsage) {
      saveContextUsageForSession(sessionId, contextUsage);
    }
  }, [
    messages,
    sessionId,
    streaming,
    sessionPendingInterrupts,
    contextUsage,
    persistClientState,
  ]);

  useEffect(() => {
    if (!persistClientState) return;
    try {
      localStorage.setItem("astro.chatRightOpen", chatRightOpen ? "1" : "0");
    } catch {
      // ignore quota / private mode
    }
  }, [chatRightOpen, persistClientState]);

  // ── Memory pending count init ─────────────────────────────────────────────
  useEffect(() => {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window))
      return;
    void (async () => {
      try {
        const rows = await invoke<{ id: string }[]>(
          "list_pending_memory_writes",
        );
        setMemoryPendingCount(rows?.length ?? 0);
      } catch {
        setMemoryPendingCount(0);
      }
    })();
  }, []);

  // ── Skills seeded toast ───────────────────────────────────────────────────
  useEffect(() => {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window))
      return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<{ installed: string[]; failed: string[] }>(
      "default-skills-seeded",
      (ev) => {
        const n = ev.payload?.installed?.length ?? 0;
        if (n <= 0) return;
        showTransientToast(
          t("skills.defaultSeeded").replace("{n}", String(n)),
          { tone: "success" },
        );
      },
    )
      .then((fn) => {
        if (disposed) fn();
        else unlisten = fn;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [t, showTransientToast]);

  // ── memory-updated event ──────────────────────────────────────────────────
  useEffect(() => {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window))
      return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<{ op?: string; content?: string; new_memories?: number }>(
      "memory-updated",
      (ev) => {
        if (!chatDisplayPrefsRef.current?.showMemory) return;
        const n = ev.payload?.new_memories ?? 0;
        const msg =
          ev.payload?.content?.trim() ||
          t("chat.toast.memoryUpdated").replace("{n}", String(n || 1));
        showTransientToast(msg);
      },
    )
      .then((fn) => {
        if (disposed) fn();
        else unlisten = fn;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [t, showTransientToast, chatDisplayPrefsRef]);

  // ── session_event: badge + memory refresh ────────────────────────────────
  useEffect(() => {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window))
      return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    type SessionEventPayload = {
      sessionId?: string | null;
      agentId?: string;
      memoryUpdated?: {
        source: string;
        target: string;
        summary: string;
        liveWritten: boolean;
      } | null;
      pendingChanged?: { pendingCount: number; reason: string } | null;
    };
    void listen<SessionEventPayload>("session_event", (ev) => {
      const p = ev.payload;
      if (p.pendingChanged) {
        setMemoryPendingCount(p.pendingChanged.pendingCount);
      }
      if (p.memoryUpdated) {
        const live = p.memoryUpdated.liveWritten;
        const summary = p.memoryUpdated.summary?.trim() || "";
        const key = `${live}:${summary}`;
        const now = Date.now();
        const prev = memoryToastDedupeRef.current;
        if (!(prev && prev.key === key && now - prev.at < 2000)) {
          memoryToastDedupeRef.current = { key, at: now };
          showTransientToast(
            live ? t("memory.toast.updated") : t("memory.toast.pending"),
          );
        }
        if (live && sessionId) {
          void (async () => {
            try {
              const settings = await invoke<{ autoRefreshOnUpdate: boolean }>(
                "get_memory_settings",
              );
              if (settings.autoRefreshOnUpdate === false) return;
              await invoke("refresh_memory", { agentId: null, sessionId });
            } catch {
              // ignore
            }
          })();
        }
      }
    })
      .then((fn) => {
        if (disposed) fn();
        else unlisten = fn;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [sessionId, showTransientToast, t]);

  // ── Cleanup event listener on unmount ────────────────────────────────────
  useEffect(() => {
    return () => {
      unlistenRef.current?.();
      if (streamRafRef.current != null) {
        cancelAnimationFrame(streamRafRef.current);
        streamRafRef.current = null;
      }
      streamPendingRef.current.clear();
    };
  }, [streamRafRef, streamPendingRef]);

  // ── Restore history ───────────────────────────────────────────────────────
  const applyRestoredHistory = useCallback(
    (
      sid: string | null,
      restored: ConversationEntry[],
      pendingInterrupts: PendingInterrupt[] = [],
      endReason?: string | null,
    ) => {
      if (restored.length === 0) return false;
      restoringRef.current = true;
      currentRunIdRef.current = null;
      setCurrentTurnId(null);
      setSessionId(sid);
      const settled = settleRestoredActivities(restored);
      setMessages(settled);
      setSessionPendingInterrupts(pendingInterrupts);
      setSessionReadOnly(!!endReason);
      setSessionEndReason(endReason ?? null);
      setEmptyMode(null);
      // 按会话恢复占用快照，避免显示上一会话数字
      const usage = loadContextUsageForSession(sid);
      setContextUsage(usage);
      if (persistClientState) {
        saveChatSession(sid, settled, pendingInterrupts, usage);
      }
      queueMicrotask(() => {
        restoringRef.current = false;
      });
      return true;
    },
    [currentRunIdRef, persistClientState],
  );

  const restoreChatHistory = useCallback(async () => {
    if (streaming || restoringRef.current) return;
    if (!isWelcomeOnly(messages)) return;
    if (pendingKeepChatBubblesRef.current != null) {
      return;
    }
    if (persistClientState && isChatCleared()) return;

    const stored = persistClientState ? loadChatSession() : null;
    if (stored && !isWelcomeOnly(stored.messages)) {
      applyRestoredHistory(
        stored.sessionId,
        stored.messages,
        stored.pendingInterrupts ?? [],
      );
      if (stored.sessionId) {
        void invoke<ResponseItemHistoryDto>("get_chat_history", {
          sessionId: stored.sessionId,
          limit: 1,
        })
          .then((h) => {
            setSessionEphemeral(!!h.ephemeral);
            setSideParentSessionId(
              h.ephemeral ? (h.parentSessionId ?? null) : null,
            );
            setSideExcludedTurnCount(
              h.ephemeral ? (h.excludedTurnCount ?? 0) : 0,
            );
            if (h.endReason) {
              setSessionReadOnly(true);
              setSessionEndReason(h.endReason);
            }
          })
          .catch(() => {
            if (!stored.ephemeral) return;
            if (persistClientState) clearChatSession();
            setMessages([]);
            setSessionId(null);
            setSessionEphemeral(false);
            setSideParentSessionId(null);
            setSideExcludedTurnCount(0);
            setEmptyMode("chat");
          });
      }
      return;
    }

    try {
      const history = await invoke<ResponseItemHistoryDto>("get_chat_history", {
        sessionId: sessionId ?? stored?.sessionId ?? null,
        limit: 200,
      });
      setSessionEphemeral(!!history.ephemeral);
      setSideParentSessionId(
        history.ephemeral ? (history.parentSessionId ?? null) : null,
      );
      setSideExcludedTurnCount(
        history.ephemeral ? (history.excludedTurnCount ?? 0) : 0,
      );
      if (history.ephemeral && history.sessionId && persistClientState) {
        saveEphemeralSessionMeta(
          history.sessionId,
          history.parentSessionId,
          history.excludedTurnCount ?? 0,
        );
      }
      if (!history.items?.length) return;
      const restored = projectResponseItemsToEntries(history.items);
      if (restored.length === 0) return;
      applyRestoredHistory(history.sessionId, restored, [], history.endReason);
    } catch {
      // keep welcome page if backend unavailable
    }
  }, [
    applyRestoredHistory,
    messages,
    sessionId,
    streaming,
    persistClientState,
  ]);

  const discardCurrentSide = useCallback(
    async (nextSessionId?: string | null) => {
      if (!sessionEphemeral || !sessionId || nextSessionId === sessionId)
        return;
      try {
        await invoke("discard_side_session", { sessionId });
      } catch (error) {
        console.warn("discard_side_session failed", error);
      }
      setSessionEphemeral(false);
      setSideParentSessionId(null);
      setSideExcludedTurnCount(0);
    },
    [sessionEphemeral, sessionId],
  );

  const clearLocalChatSurface = useCallback(() => {
    unlistenRef.current?.();
    unlistenRef.current = null;
    clearStreamBuffers();
    if (persistClientState) clearChatSession();
    pendingKeepChatBubblesRef.current = null;
    setSessionId(null);
    setSessionEphemeral(false);
    setSideParentSessionId(null);
    setSideExcludedTurnCount(0);
    setAttachments((prev) => {
      for (const a of prev) {
        if (a.previewUrl) URL.revokeObjectURL(a.previewUrl);
      }
      return [];
    });
    setStreaming(false);
    setStreamPaused(false);
    turnInFlightRef.current = false;
    setTurnInFlight(false);
    setTokenUsage(null);
    setContextUsage(null);
    activeAssistantIdRef.current = null;
    setStatus("ready");
    setStatusPhase("ready");
    setStatusDetail(null);
    setFocusMessageId(null);
    setMessages([]);
    setSessionPendingInterrupts([]);
    setSessionReadOnly(false);
    setSessionEndReason(null);
    currentRunIdRef.current = null;
    setCurrentTurnId(null);
    setInput("");
    setEmptyMode("chat");
    setNav("chat");
    setQueuedFollowUps((prev) => {
      for (const q of prev) {
        for (const a of q.attachments) {
          if (a.previewUrl) URL.revokeObjectURL(a.previewUrl);
        }
      }
      return [];
    });
    setModeSwitchPrompt(null);
    modeSwitchArmedRef.current = false;
    const wt = sessionWorktreeRef.current;
    if (wt) {
      void invoke("cleanup_task_worktree", {
        worktreeId: wt.id,
      }).catch(() => {});
      sessionWorktreeRef.current = null;
    }
  }, [
    activeAssistantIdRef,
    clearStreamBuffers,
    currentRunIdRef,
    persistClientState,
    setNav,
  ]);

  /** 永久删除当前会话前：先取消流并丢弃本地监听，避免 ghost token。 */
  const prepareDeleteCurrentSession = useCallback(async () => {
    if (!sessionId) return;
    streamGenRef.current += 1;
    unlistenRef.current?.();
    unlistenRef.current = null;
    if (streamRafRef.current != null) {
      cancelAnimationFrame(streamRafRef.current);
      streamRafRef.current = null;
    }
    if (toolDeltaRafRef.current != null) {
      cancelAnimationFrame(toolDeltaRafRef.current);
      toolDeltaRafRef.current = null;
    }
    setStreaming(false);
    setStreamPaused(false);
    try {
      await invoke("chat_control", { sessionId, action: "cancel" });
    } catch (e) {
      console.warn("chat_control cancel before delete failed", e);
    }
  }, [sessionId]);

  useEffect(() => {
    if (nav !== "chat") return;
    void restoreChatHistory();
  }, [nav, restoreChatHistory]);

  // ── auto-compact after stream ends ───────────────────────────────────────
  const runCompactSession = useCallback(async () => {
    if (compactingRef.current || isCompacting) {
      showTransientToast(t("chat.compactAlreadyRunning"), { tone: "warning" });
      return;
    }
    if (streaming) {
      showTransientToast(t("chat.compactBlockedStreaming"), {
        tone: "warning",
      });
      return;
    }
    if (sessionPendingInterrupts.length > 0) {
      showTransientToast(t("chat.compactBlockedInterrupt"), {
        tone: "warning",
      });
      return;
    }
    if (!sessionId) {
      showTransientToast(t("chat.compactFailed", { error: "no session" }), {
        tone: "error",
      });
      return;
    }

    compactingRef.current = true;
    setIsCompacting(true);
    let splitNewId: string | null = null;
    try {
      const res = await invoke<{
        newSessionId: string;
        summaryPreview: string;
        degraded: boolean;
      }>(
        "compact_chat_session",
        // keepTailBubbles 缺省时由后端读 compression.keep_tail_bubbles
        { sessionId, keepTailBubbles: null, focus: null },
      );
      splitNewId = res.newSessionId;

      unlistenRef.current?.();
      unlistenRef.current = null;
      clearStreamBuffers();
      setStreaming(false);
      setStreamPaused(false);
      setFocusMessageId(null);
      currentRunIdRef.current = null;
      setCurrentTurnId(null);
      setSessionPendingInterrupts([]);
      setSessionReadOnly(false);
      setSessionEndReason(null);
      setSessionId(res.newSessionId);
      setEmptyMode(null);
      if (persistClientState) saveChatSession(res.newSessionId, [], []);

      try {
        const history = await invoke<ResponseItemHistoryDto>(
          "get_chat_history",
          {
            sessionId: res.newSessionId,
            limit: 200,
          },
        );
        const restored = projectResponseItemsToEntries(history.items ?? []);
        if (!applyRestoredHistory(res.newSessionId, restored, [], null)) {
          setMessages(restored);
          if (persistClientState)
            saveChatSession(res.newSessionId, restored, []);
        }
        showTransientToast(
          res.degraded ? t("chat.compactDegraded") : t("chat.compactDone"),
          { tone: res.degraded ? "warning" : "success" },
        );
      } catch (histErr) {
        showTransientToast(
          t("chat.compactHistoryFailed", {
            error:
              histErr instanceof Error
                ? histErr.message
                : String(histErr ?? "error"),
          }),
          { tone: "warning" },
        );
      }
    } catch (e) {
      if (!splitNewId) {
        showTransientToast(
          t("chat.compactFailed", {
            error: e instanceof Error ? e.message : String(e ?? "error"),
          }),
          { tone: "error" },
        );
      } else {
        showTransientToast(
          t("chat.compactHistoryFailed", {
            error: e instanceof Error ? e.message : String(e ?? "error"),
          }),
          { tone: "warning" },
        );
      }
    } finally {
      compactingRef.current = false;
      setIsCompacting(false);
    }
  }, [
    streaming,
    isCompacting,
    sessionPendingInterrupts,
    sessionId,
    clearStreamBuffers,
    applyRestoredHistory,
    showTransientToast,
    t,
    currentRunIdRef,
  ]);

  // 会话级压实仅由用户手动触发（/compact 或菜单）；前端不做自动压实。
  // 上下文接近上限时后端发 recommendCompact，仅 toast 建议，见 useSend。

  // ── Stream controls ───────────────────────────────────────────────────────
  const pauseStream = useCallback(async () => {
    if (!sessionId || !streaming || streamPaused) return;
    try {
      await invoke("chat_control", { sessionId, action: "pause" });
      setStreamPaused(true);
    } catch (e) {
      console.warn("chat_control pause failed", e);
    }
  }, [sessionId, streaming, streamPaused]);

  const resumeStream = useCallback(async () => {
    if (!sessionId || !streaming || !streamPaused) return;
    try {
      await invoke("chat_control", { sessionId, action: "resume" });
      setStreamPaused(false);
    } catch (e) {
      console.warn("chat_control resume failed", e);
    }
  }, [sessionId, streaming, streamPaused]);

  const stopStream = useCallback(async () => {
    if (!streaming && !turnInFlightRef.current) {
      setSessionPendingInterrupts([]);
      return;
    }
    if (!sessionId) {
      setStreaming(false);
      setStreamPaused(false);
      turnInFlightRef.current = false;
      setTurnInFlight(false);
      setSessionPendingInterrupts([]);
      return;
    }
    streamGenRef.current += 1;
    unlistenRef.current?.();
    unlistenRef.current = null;
    if (streamRafRef.current != null) {
      cancelAnimationFrame(streamRafRef.current);
      flushStreamTokens();
    }
    if (toolDeltaRafRef.current != null) {
      cancelAnimationFrame(toolDeltaRafRef.current);
      flushToolDeltas();
    }
    try {
      await invoke("chat_control", { sessionId, action: "cancel" });
    } catch (e) {
      console.warn("chat_control cancel failed", e);
    }
    const aid = activeAssistantIdRef.current;
    if (aid) {
      const endedAt = Date.now();
      const usage = pendingUsageRef.current.get(aid);
      const genStart =
        firstTokenRef.current.get(aid) ?? streamStartRef.current.get(aid);
      const tokensPerSec =
        usage && genStart != null
          ? calcTokensPerSec(usage.completionTokens, endedAt - genStart)
          : undefined;
      setMessages((prev) =>
        prev.map((m) => {
          if (m.id !== aid) return m;
          return sealOpenReasoning(
            {
              ...m,
              usage: usage ?? m.usage,
              tokensPerSec: tokensPerSec ?? m.tokensPerSec,
              generationDurationSec:
                m.generationDurationSec ??
                (m.generationStartedAt != null
                  ? elapsedSecSince(m.generationStartedAt, endedAt)
                  : undefined),
              generationStartedAt: undefined,
            },
            endedAt,
          );
        }),
      );
      pendingUsageRef.current.delete(aid);
    }
    activeAssistantIdRef.current = null;
    clearStreamBuffers();
    setStreaming(false);
    setStreamPaused(false);
    turnInFlightRef.current = false;
    setTurnInFlight(false);
    setSessionPendingInterrupts([]);
    setStatus("ready");
    setStatusPhase("ready");
  }, [
    sessionId,
    streaming,
    clearStreamBuffers,
    flushStreamTokens,
    flushToolDeltas,
    streamGenRef,
    streamRafRef,
    toolDeltaRafRef,
    activeAssistantIdRef,
    pendingUsageRef,
    firstTokenRef,
    streamStartRef,
  ]);

  stopStreamRef.current = stopStream;

  /** 长任务空闲巡检：无 token/tool 活动超阈值且有排队时，暂停当前回合以出队 */
  const QUEUE_CHECKPOINT_IDLE_MS = 60_000;
  useEffect(() => {
    if (!turnInFlight) {
      checkpointFiredForTurnRef.current = false;
      return;
    }
    const timer = window.setInterval(() => {
      if (!turnInFlightRef.current) return;
      if (checkpointFiredForTurnRef.current) return;
      if (sessionPendingInterrupts.length > 0) return;
      if (queuedFollowUps.length === 0) return;
      if (queueDrainLockRef.current) return;
      const last = lastStreamActivityAtRef.current;
      if (!last || Date.now() - last < QUEUE_CHECKPOINT_IDLE_MS) return;
      checkpointFiredForTurnRef.current = true;
      showTransientToast(t("chat.queue.checkpointDrain"), { tone: "warning" });
      void stopStream().then(() => {
        setQueueKick((k) => k + 1);
      });
    }, 5_000);
    return () => window.clearInterval(timer);
  }, [
    turnInFlight,
    queuedFollowUps.length,
    sessionPendingInterrupts.length,
    stopStream,
    showTransientToast,
    t,
  ]);

  // ── Message operations ────────────────────────────────────────────────────
  const undoLastExchange = useCallback(() => {
    if (streaming) return;
    setMessages((prev) => {
      let lastUser = -1;
      for (let i = prev.length - 1; i >= 0; i -= 1) {
        if (prev[i].role === "user" && prev[i].id !== "welcome") {
          lastUser = i;
          break;
        }
      }
      if (lastUser < 0) {
        queueMicrotask(() => showTransientToast(t("chat.slashUndoEmpty")));
        return prev;
      }
      let end = lastUser + 1;
      while (end < prev.length && prev[end].role === "assistant") end += 1;
      const next = [...prev.slice(0, lastUser), ...prev.slice(end)];
      if (next.length === 0) {
        queueMicrotask(() => setEmptyMode("chat"));
      }
      return next;
    });
  }, [streaming, showTransientToast, t]);

  const editUserMessage = useCallback(
    async (messageId: string, content: string): Promise<boolean> => {
      if (streaming || turnInFlight) return false;
      const idx = findLastUserEntryIndex(messages);
      if (idx < 0 || messages[idx]?.id !== messageId) return false;
      const userMsg = messages[idx];
      const nextText = content.trim();
      if (!nextText || nextText === userMsg.content.trim()) return false;
      const bubbleStart = countChatBubbles(messages.slice(0, idx));
      pendingKeepChatBubblesRef.current = bubbleStart;
      setSessionPendingInterrupts([]);
      const accepted = await sendImmediate({
        text: nextText,
        attachments: (userMsg.attachments ?? []).map((attachment) => ({
          ...attachment,
        })),
        truncateTo: idx,
        skipUserAppend: false,
        reuseUserId: userMsg.id,
      });
      if (!accepted) pendingKeepChatBubblesRef.current = null;
      return accepted;
    },
    [messages, sendImmediate, streaming, turnInFlight],
  );

  const branchMessage = useCallback(
    async (messageId: string) => {
      if (streaming) return;
      const idx = messages.findIndex((m) => m.id === messageId);
      if (idx < 0) return;
      const keep = messages.slice(0, idx + 1).filter((m) => m.id !== "welcome");
      if (keep.length === 0) return;

      const newId = crypto.randomUUID();
      const sourceId = sessionId;

      if (sourceId) {
        try {
          await invoke<string>("fork_chat_session", {
            sourceSessionId: sourceId,
            keepChatBubbles: keep.length,
            newSessionId: newId,
          });
        } catch (e) {
          showTransientToast(
            t("chat.branchFailed", {
              error: e instanceof Error ? e.message : String(e ?? "error"),
            }),
          );
          return;
        }
      }

      unlistenRef.current?.();
      unlistenRef.current = null;
      clearStreamBuffers();
      setSessionPendingInterrupts([]);
      setStreaming(false);
      setStreamPaused(false);
      setFocusMessageId(null);
      currentRunIdRef.current = null;
      setCurrentTurnId(null);
      setSessionId(newId);
      setMessages(keep);
      setSessionReadOnly(false);
      setSessionEndReason(null);
      setEmptyMode(null);
      if (persistClientState) saveChatSession(newId, keep, []);
      dispatchSessionsChanged();
      showTransientToast(t("chat.branchDone"), { tone: "success" });
    },
    [
      messages,
      streaming,
      sessionId,
      clearStreamBuffers,
      persistClientState,
      showTransientToast,
      t,
      currentRunIdRef,
    ],
  );

  // ── HITL UI action ────────────────────────────────────────────────────────
  const onUiAction = useCallback(
    async (
      messageId: string,
      name: string,
      context: Record<string, unknown>,
    ) => {
      const parallelTask = parallelTasks.find(
        (t) =>
          t.assistantMessageId === messageId &&
          t.status === "waiting" &&
          (t.pendingInterrupts?.length ?? 0) > 0,
      );
      const interrupts = parallelTask?.pendingInterrupts?.length
        ? parallelTask.pendingInterrupts
        : sessionPendingInterrupts;
      if (!activeProvider || interrupts.length === 0) return;

      const isLocationHitl = interrupts.some(
        (p) => p.reason === "location_required",
      );

      let payload: Record<string, unknown>;
      if (name === "share_location") {
        if (
          typeof navigator === "undefined" ||
          !navigator.geolocation?.getCurrentPosition
        ) {
          showTransientToast(t("chat.location.geoUnavailable"), {
            tone: "error",
          });
          return;
        }
        try {
          const pos = await new Promise<GeolocationPosition>(
            (resolve, reject) => {
              navigator.geolocation.getCurrentPosition(resolve, reject, {
                enableHighAccuracy: true,
                timeout: 15_000,
                maximumAge: 60_000,
              });
            },
          );
          payload = {
            latitude: pos.coords.latitude,
            longitude: pos.coords.longitude,
            accuracy_m: pos.coords.accuracy,
          };
        } catch {
          showTransientToast(t("chat.location.geoFailed"), { tone: "error" });
          return;
        }
      } else if (name === "choose_city") {
        const cityRaw = context.city;
        const city = typeof cityRaw === "string" ? cityRaw.trim() : "";
        if (!city) {
          showTransientToast(t("chat.location.cityRequired"), {
            tone: "warning",
          });
          return;
        }
        payload = { city };
      } else if (name === "approve") {
        payload = { approved: true };
      } else if (name === "approve_always") {
        payload = { approved: true, always: true, scope: "exact" };
      } else if (name === "approve_type") {
        payload = { approved: true, always: true, scope: "type" };
      } else if (name === "allow_once") {
        payload = { scope: "allow_once" };
      } else if (name === "allow_session") {
        payload = { scope: "allow_session" };
      } else if (name === "allow_always") {
        payload = { scope: "allow_always" };
      } else if (name === "deny") {
        const isNetworkApproval = interrupts.some(
          (p) => p.reason === "network_approval",
        );
        payload = isLocationHitl
          ? { denied: true }
          : isNetworkApproval
            ? { scope: "deny" }
            : { approved: false };
      } else if (name === "choose") {
        const answers = context.answers;
        if (answers && typeof answers === "object" && !Array.isArray(answers)) {
          const confirmAnswer = (answers as Record<string, string>).confirm;
          if (typeof confirmAnswer === "string") {
            const lower = confirmAnswer.toLowerCase();
            if (lower.includes("approve") && lower.includes("always")) {
              payload = { approved: true, always: true };
            } else if (lower.includes("approve")) {
              payload = { approved: true };
            } else {
              payload = { approved: false };
            }
          } else {
            const value =
              typeof context.value === "string" && context.value.trim()
                ? context.value.trim()
                : Object.values(answers as Record<string, unknown>)
                    .filter((v) => typeof v === "string" && v.trim())
                    .join("；");
            if (!value) return;
            payload = { answers, value };
          }
        } else {
          const value = context.value;
          if (typeof value !== "string" || !value.trim()) return;
          payload = { value };
        }
      } else {
        payload = { ...context };
      }

      const elicitation = interrupts.find(
        (item) => item.reason === "elicitation",
      );
      if (elicitation) {
        if (parallelTask) {
          await resumeParallelHitl(messageId, payload, name);
          return;
        }
        const metadataPayload = elicitation.metadata?.payload;
        const metadata =
          metadataPayload &&
          typeof metadataPayload === "object" &&
          !Array.isArray(metadataPayload)
            ? (metadataPayload as Record<string, unknown>)
            : undefined;
        const serverName =
          typeof metadata?.server_name === "string" ? metadata.server_name : "";
        const targetSessionId = sessionId;
        if (!targetSessionId || !serverName) {
          showTransientToast("MCP elicitation routing metadata is missing", {
            tone: "error",
          });
          return;
        }
        try {
          const action = resolveElicitationAction(name);
          await invoke("resolve_elicitation", {
            sessionId: targetSessionId,
            serverName,
            requestId: elicitationRequestId(elicitation),
            action,
            contentJson:
              action !== "accept"
                ? null
                : JSON.stringify(buildElicitationContent(elicitation, payload)),
            metaJson: null,
          });
          const remaining = sessionPendingInterrupts.filter(
            (interrupt) => interrupt.id !== elicitation.id,
          );
          setSessionPendingInterrupts(remaining);
          if (remaining.length === 0) {
            setStreaming(true);
            setStatus("busy");
            setStatusPhase("generating");
          }
        } catch (error) {
          showTransientToast(
            error instanceof Error ? error.message : String(error),
            { tone: "error" },
          );
        }
        return;
      }

      if (parallelTask) {
        await resumeParallelHitl(messageId, payload, name);
        return;
      }

      const resumeJson = JSON.stringify(
        sessionPendingInterrupts.map((p) => ({
          interrupt_id: p.id,
          status: "resolved",
          payload,
        })),
      );
      setMessages((prev) =>
        prev.map((m) => {
          if (m.id !== messageId) return m;
          return {
            ...m,
            uiSurfaces: m.uiSurfaces?.map((s) => ({
              ...s,
              status: "resolved" as const,
            })),
          };
        }),
      );
      setSessionPendingInterrupts([]);
      if (!sessionId) {
        showTransientToast(t("chat.interrupt.pending"));
        return;
      }
      try {
        setStreaming(true);
        setStreamPaused(false);
        setStatus("busy");
        setStatusPhase("generating");
        await invoke("interrupt_resume", { sessionId, resumeJson });
      } catch (e) {
        setStreaming(false);
        showTransientToast(
          e instanceof Error ? e.message : String(e ?? "HITL resume failed"),
        );
      }
    },
    [
      activeProvider,
      parallelTasks,
      resumeParallelHitl,
      sessionPendingInterrupts,
      sessionId,
      showTransientToast,
      t,
      setStreaming,
      setStreamPaused,
      setStatus,
      setStatusPhase,
    ],
  );

  // ── Reset / New session ───────────────────────────────────────────────────
  const resetChatSurface = useCallback(() => {
    const sid = sessionId;
    if (sessionEphemeral && sid) {
      void discardCurrentSide(null);
    } else if (
      sid &&
      typeof window !== "undefined" &&
      "__TAURI_INTERNALS__" in window
    ) {
      void invoke("chat_control", { sessionId: sid, action: "new_chat" }).catch(
        (e) => console.warn("chat_control new_chat failed", e),
      );
    }
    clearLocalChatSurface();
  }, [sessionId, sessionEphemeral, discardCurrentSide, clearLocalChatSurface]);

  const confirmIfStreaming = useCallback(async () => {
    if (!streaming && !turnInFlight) return true;
    return confirm({
      title: t("chat.newSession"),
      message: t("chat.newSessionStreamingConfirm"),
    });
  }, [streaming, turnInFlight, t, confirm]);

  const startNewChat = useCallback(async () => {
    if (!(await confirmIfStreaming())) return;
    resetChatSurface();
    setInput("");
    setEmptyMode("chat");
  }, [confirmIfStreaming, resetChatSurface]);

  const resetSchedulingSurface = useCallback(() => {
    setQueuedFollowUps((prev) => {
      for (const q of prev) {
        for (const a of q.attachments) {
          if (a.previewUrl) URL.revokeObjectURL(a.previewUrl);
        }
      }
      return [];
    });
    setModeSwitchPrompt(null);
    modeSwitchArmedRef.current = false;
    modeSwitchPromptRef.current = null;
    turnInFlightRef.current = false;
    setTurnInFlight(false);
    checkpointFiredForTurnRef.current = false;
    steeringQueueIdsRef.current.clear();
    queueFailedIdRef.current = null;
    setStreaming(false);
    setStreamPaused(false);
    const wt = sessionWorktreeRef.current;
    if (wt) {
      void invoke("cleanup_task_worktree", {
        worktreeId: wt.id,
      }).catch(() => {});
      sessionWorktreeRef.current = null;
    }
  }, []);

  // ── Open session from file space ──────────────────────────────────────────
  const openSessionFromFilespace = useCallback(
    async (targetSessionId: string, messageId?: string | null) => {
      try {
        await discardCurrentSide(targetSessionId);
        resetSchedulingSurface();
        const hist = await invoke<ResponseItemHistoryDto>("get_chat_history", {
          sessionId: targetSessionId,
          limit: 200,
        });
        const restored = projectResponseItemsToEntries(hist.items ?? []);
        const endReason = hist.endReason ?? null;
        const resolvedSessionId = hist.sessionId ?? targetSessionId;
        if (restored.length > 0) {
          applyRestoredHistory(resolvedSessionId, restored, [], endReason);
        } else {
          currentRunIdRef.current = null;
          setCurrentTurnId(null);
          setSessionId(resolvedSessionId);
          setSessionReadOnly(!!endReason);
          setSessionEndReason(endReason);
          const usage = loadContextUsageForSession(resolvedSessionId);
          setContextUsage(usage);
        }
        setSessionEphemeral(!!hist.ephemeral);
        setSideParentSessionId(
          hist.ephemeral ? (hist.parentSessionId ?? null) : null,
        );
        setSideExcludedTurnCount(
          hist.ephemeral ? (hist.excludedTurnCount ?? 0) : 0,
        );
        if (hist.ephemeral && persistClientState) {
          saveEphemeralSessionMeta(
            resolvedSessionId,
            hist.parentSessionId,
            hist.excludedTurnCount ?? 0,
          );
        }
        const canFocus =
          !!messageId && restored.some((m) => m.id === messageId);
        setFocusMessageId(canFocus ? messageId! : null);
        setEmptyMode(null);
        setNav("chat");
      } catch (e) {
        setStatus("error");
        setStatusPhase("error");
        setStatusDetail(String(e));
      }
    },
    [
      applyRestoredHistory,
      currentRunIdRef,
      setNav,
      resetSchedulingSurface,
      discardCurrentSide,
      persistClientState,
    ],
  );

  // ── Attach artifacts ──────────────────────────────────────────────────────
  const attachArtifactsToChat = useCallback(
    async (files: ArtifactDto[], mode: "new" | "current") => {
      const usable = files.filter((f) => !f.missing);
      const converted: ChatAttachment[] = [];
      const fails: string[] = [];

      for (const f of usable) {
        try {
          const dto = await invoke<{
            name: string;
            mime: string;
            size: number;
            base64: string;
          }>("read_file_base64", { path: f.path });
          const kind = kindFromMime(dto.mime, dto.name);
          const shouldInline =
            (kind === "image" && dto.size <= MAX_INLINE_BYTES) ||
            (kind === "file" &&
              dto.size <= 256 * 1024 &&
              (dto.mime.startsWith("text/") ||
                /\.(txt|md|json|csv|xml|yaml|yml|toml|rs|ts|tsx|js|py|html|css)$/i.test(
                  dto.name,
                )));
          let previewUrl: string | undefined;
          if (kind === "image" || kind === "video") {
            try {
              previewUrl = convertFileSrc(f.path);
            } catch {
              previewUrl = undefined;
            }
          }
          converted.push({
            id: `att-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
            name: dto.name,
            mime: dto.mime,
            kind,
            size: dto.size,
            previewUrl,
            dataBase64: shouldInline ? dto.base64 : undefined,
          });
        } catch (e) {
          fails.push(`${f.name}: ${String(e)}`);
        }
      }

      if (mode === "new") {
        if (!(await confirmIfStreaming())) return;
        resetChatSurface();
        setInput("");
        setEmptyMode("chat");
        const capped = converted.slice(0, MAX_ATTACHMENTS);
        setAttachments(capped);
        setNav("chat");
        if (fails.length > 0) {
          setStatusDetail(
            t("filespace.toast.partialFail", {
              ok: String(converted.length),
              fail: String(fails.length),
              detail: fails.slice(0, 2).join("; "),
            }),
          );
        } else if (converted.length > MAX_ATTACHMENTS) {
          setStatusDetail(
            t("filespace.toast.attachTruncated", {
              max: String(MAX_ATTACHMENTS),
              n: String(capped.length),
            }),
          );
        }
        return;
      }

      let truncated = false;
      let addedCount = 0;
      setAttachments((prev) => {
        const room = MAX_ATTACHMENTS - prev.length;
        const added = converted.slice(0, Math.max(0, room));
        addedCount = added.length;
        truncated = converted.length > room;
        return [...prev, ...added];
      });
      setNav("chat");
      if (fails.length > 0) {
        setStatusDetail(
          t("filespace.toast.partialFail", {
            ok: String(converted.length),
            fail: String(fails.length),
            detail: fails.slice(0, 2).join("; "),
          }),
        );
      } else if (truncated) {
        setStatusDetail(
          t("filespace.toast.attachTruncated", {
            max: String(MAX_ATTACHMENTS),
            n: String(addedCount),
          }),
        );
      }
    },
    [confirmIfStreaming, resetChatSurface, t, setNav],
  );

  return {
    // state
    messages,
    emptyMode,
    input,
    attachments,
    queuedFollowUps,
    parallelTasks,
    modeSwitchPrompt,
    streaming,
    primaryStreaming: streaming,
    turnInFlight,
    streamPaused,
    tokenUsage,
    contextUsage,
    sessionId,
    sessionEphemeral,
    sideParentSessionId,
    sideExcludedTurnCount,
    sessionPendingInterrupts,
    sessionReadOnly,
    sessionEndReason,
    currentTurnId,
    completionCelebrationId,
    focusMessageId,
    isCompacting,
    status,
    statusPhase,
    statusDetail,
    memoryPendingCount,
    chatRightOpen,
    chatRightTab,
    generatingPreview,
    browserPreview,
    controlBrowser,
    applyBrowserResult,
    // setters needed by App
    setInput,
    setAttachments,
    setMemoryPendingCount,
    setChatRightOpen,
    setChatRightTab,
    setFocusMessageId,
    setStatusDetail,
    dismissBrowserPreview,
    // callbacks
    send,
    approveModeSwitch,
    dismissModeSwitch,
    removeQueuedFollowUp,
    updateQueuedFollowUpText,
    moveQueuedFollowUp,
    steerQueuedFollowUp,
    openQueuedFollowUpInNewTask,
    closeQueuedFollowUps,
    cancelParallelTask,
    clearSettledParallel,
    writeParallelSummary,
    pauseStream,
    resumeStream,
    stopStream,
    undoLastExchange,
    editUserMessage,
    branchMessage,
    runCompactSession,
    onUiAction,
    resetChatSurface,
    startNewChat,
    openSessionFromFilespace,
    attachArtifactsToChat,
    applyRestoredHistory,
    confirmIfStreaming,
    prepareDeleteCurrentSession,
    clearDeletedCurrentSession: clearLocalChatSurface,
  };
}
