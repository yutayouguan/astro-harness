import { useCallback, useRef } from "react";
import type {
  Dispatch,
  MutableRefObject,
  RefObject,
  SetStateAction,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  applyActivityUpsert,
  applySurfaceUpsert,
  parseActivityOperations,
  reconcileReasoning,
  reconcileText,
  sealOpenReasoning,
} from "../../lib/chat/chatTimeline";
import { elapsedSecSince } from "../../lib/chat/elapsedSec";
import { normalizeContextUsageEvent } from "../../lib/chat/contextUsage";
import { saveContextUsageForSession } from "../../lib/chat/chatSessionStore";
import { upsertAsyncAgentUpdate } from "../../lib/chat/asyncAgentUpdate";
import {
  parseModeSwitchResult,
  type ChatInteractionMode,
  type ModeSwitchRequest,
} from "../../lib/chat/chatMode";
import { resolveComposerTurn } from "../../lib/chat/composerResolve";
import { parseHitlRunFinished } from "../../lib/chat/hitlRunFinished";
import { resolveTaskCompletion } from "../../lib/chat/taskCompletion";
import { normalizeChatWebAction } from "../../lib/chat/webActivity";
import {
  isLiveActivityStatus,
  isSettledActivityStatus,
  resolveToolActivityStatus,
} from "../../lib/chat/toolActivityStatus";
import {
  loadPickerGlobals,
  loadModelPrefs,
  modelPrefsToApi,
} from "../../lib/model/modelPrefs";
import {
  loadModelCandidates,
  selectAutoModel,
} from "../../lib/model/autoModelSelect";
import { shouldShowThinkingControls } from "../../lib/chat/shouldShowThinkingControls";
import type {
  ChatActivity,
  ChatAttachment,
  ChatEmptyMode,
  ConversationEntry,
  TurnTokenUsage,
  PendingInterrupt,
  ProviderDto,
  UiSurface,
} from "../../types";
import type { ContextUsageSnapshot } from "../../lib/chat/contextUsage";
import type { GeneratingPreviewApi } from "./useGeneratingPreview";
import type { BrowserPreviewApi } from "./useBrowserPreview";
import { isCodePath } from "../../lib/media/parseGeneratedMedia";
import type { ChatDisplayPrefs } from "./useChatDisplayPrefs";
import type { ShowToastOptions } from "../ui/useTransientToast";
import type { MessageKey } from "../../i18n/messages";

type TFn = (key: MessageKey, vars?: Record<string, string>) => string;
type ShowToastFn = (msg: string, opts?: ShowToastOptions) => void;
type StatusPhase = "ready" | "connecting" | "generating" | "error";

export interface SendOpts {
  text?: string;
  attachments?: ChatAttachment[];
  truncateTo?: number;
  skipUserAppend?: boolean;
  reuseUserId?: string;
  resumeJson?: string;
  allowEmpty?: boolean;
  /** 覆盖当前 UI 模式（例如刚批准 Plan→Agent 时） */
  interactionMode?: ChatInteractionMode;
}

export interface UseSendDeps {
  projectId: string;
  // composer state
  input: string;
  attachments: ChatAttachment[];
  // guards
  streaming: boolean;
  isCompacting: boolean;
  sessionReadOnly: boolean;
  sessionEndReason: string | null;
  // provider/model
  activeProvider: ProviderDto | undefined;
  providers: ProviderDto[];
  // session
  sessionId: string | null;
  emptyMode: ChatEmptyMode;
  sessionPendingInterrupts: PendingInterrupt[];
  // config
  chatMode: ChatInteractionMode;
  t: TFn;
  chatDisplayPrefsRef: RefObject<ChatDisplayPrefs>;
  // stream buffer callbacks
  clearStreamBuffers: () => void;
  enqueueStreamToken: (messageId: string, token: string) => void;
  enqueueStreamReasoning: (messageId: string, token: string) => void;
  enqueueToolDelta: (
    messageId: string,
    delta: { index: number; id?: string; name?: string; arguments?: string },
  ) => void;
  flushStreamTokens: () => void;
  flushToolDeltas: () => void;
  settleMessageUsage: (messageId: string, endedAt?: number) => void;
  /** 生成中文件实时预览 */
  generatingPreviewApi: GeneratingPreviewApi;
  /** 任务绑定浏览器悬浮预览 */
  browserPreviewApi: BrowserPreviewApi;
  // stream buffer refs
  streamGenRef: MutableRefObject<number>;
  currentRunIdRef: MutableRefObject<string | null>;
  activeAssistantIdRef: MutableRefObject<string | null>;
  streamStartRef: MutableRefObject<Map<string, number>>;
  firstTokenRef: MutableRefObject<Map<string, number>>;
  pendingUsageRef: MutableRefObject<Map<string, TurnTokenUsage>>;
  streamPendingRef: MutableRefObject<Map<string, string>>;
  toolDeltaIdsRef: MutableRefObject<Map<string, string>>;
  toolDeltaRafRef: MutableRefObject<number | null>;
  streamRafRef: MutableRefObject<number | null>;
  // session refs
  unlistenRef: MutableRefObject<UnlistenFn | null>;
  compactingRef: MutableRefObject<boolean>;
  pendingKeepChatBubblesRef: MutableRefObject<number | null>;
  /** recommendCompact toast 冷却（ms epoch） */
  lastRecommendCompactToastAtRef: MutableRefObject<number>;
  // setters
  setMessages: Dispatch<SetStateAction<ConversationEntry[]>>;
  setSessionId: Dispatch<SetStateAction<string | null>>;
  setStreaming: Dispatch<SetStateAction<boolean>>;
  setStreamPaused: Dispatch<SetStateAction<boolean>>;
  setTokenUsage: Dispatch<SetStateAction<TurnTokenUsage | null>>;
  setContextUsage: Dispatch<SetStateAction<ContextUsageSnapshot | null>>;
  setStatus: Dispatch<SetStateAction<"ready" | "busy" | "error">>;
  setStatusPhase: Dispatch<SetStateAction<StatusPhase>>;
  setStatusDetail: Dispatch<SetStateAction<string | null>>;
  setEmptyMode: Dispatch<SetStateAction<ChatEmptyMode>>;
  setInput: Dispatch<SetStateAction<string>>;
  setAttachments: Dispatch<SetStateAction<ChatAttachment[]>>;
  setSessionPendingInterrupts: Dispatch<SetStateAction<PendingInterrupt[]>>;
  setCurrentTurnId: Dispatch<SetStateAction<string | null>>;
  showTransientToast: ShowToastFn;
  /** 主会话整轮未结束（含 HITL 停顿）；供队列软边界 */
  turnInFlightRef: MutableRefObject<boolean>;
  /** 提交前置解析单飞锁；防止 streaming state 提交前重复启动。 */
  sendStartLockRef: MutableRefObject<boolean>;
  setTurnInFlight: Dispatch<SetStateAction<boolean>>;
  /** 流式活动时间戳（token/tool）；供长任务 idle checkpoint */
  lastStreamActivityAtRef: MutableRefObject<number>;
  /** Agent 会话级 worktree（按 sessionId 复用） */
  /** 本轮首次检测到模式切换请求时记录（流结束后再弹授权条） */
  onModeSwitchDetected?: (req: ModeSwitchRequest) => void;
  /** 流正常结束后，若本轮有模式切换请求则提示 UI */
  onModeSwitchPrompt?: (req: ModeSwitchRequest) => void;
  /** A steered queue item is durable in the active turn history. */
  onUserInputCommitted?: (clientMessageId: string) => void;
  /** 整轮成功完成；错误、中断和 HITL 等待均不触发。 */
  onTurnSucceeded?: () => void;
  /** 独立临时聊天不覆盖主聊天的 context usage 快照。 */
  persistContextUsage?: boolean;
}

function calcTokensPerSec(
  completionTokens: number,
  durationMs: number,
): number | undefined {
  if (completionTokens <= 0 || durationMs <= 0) return undefined;
  const sec = Math.max(0.1, durationMs / 1000);
  return Math.round((completionTokens / sec) * 10) / 10;
}

export function useSend(deps: UseSendDeps) {
  const depsRef = useRef(deps);
  depsRef.current = deps;

  /** @returns 是否已开始流式（`setStreaming(true)` 之后）；供 follow-up 队列判断是否出队成功 */
  const send = useCallback(
    async (opts?: SendOpts): Promise<boolean> => {
      const {
        projectId,
        input,
        attachments,
        streaming,
        isCompacting,
        sessionReadOnly,
        sessionEndReason,
        activeProvider,
        providers,
        sessionId,
        emptyMode,
        sessionPendingInterrupts,
        chatMode,
        t,
        chatDisplayPrefsRef,
        clearStreamBuffers,
        enqueueStreamToken,
        enqueueStreamReasoning,
        enqueueToolDelta,
        flushStreamTokens,
        flushToolDeltas,
        generatingPreviewApi,
        browserPreviewApi,
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
        unlistenRef,
        compactingRef,
        pendingKeepChatBubblesRef,
        lastRecommendCompactToastAtRef,
        setMessages,
        setSessionId,
        setStreaming,
        setStreamPaused,
        setTokenUsage,
        setContextUsage,
        setStatus,
        setStatusPhase,
        setStatusDetail,
        setEmptyMode,
        setInput,
        setAttachments,
        setSessionPendingInterrupts,
        setCurrentTurnId,
        showTransientToast,
        turnInFlightRef,
        sendStartLockRef,
        setTurnInFlight,
        persistContextUsage = true,
        lastStreamActivityAtRef,
        onModeSwitchDetected,
        onModeSwitchPrompt,
        onUserInputCommitted,
        onTurnSucceeded,
      } = depsRef.current;

      const markTurnEnded = () => {
        turnInFlightRef.current = false;
        setTurnInFlight(false);
      };
      const touchActivity = () => {
        lastStreamActivityAtRef.current = Date.now();
      };

      let pendingModeSwitch: ModeSwitchRequest | null = null;
      const text = (opts?.text ?? input).trim();
      const pending = opts?.attachments ?? attachments;
      const resumeJson = opts?.resumeJson?.trim() ?? "";

      if (sessionPendingInterrupts.length > 0 && !resumeJson) {
        showTransientToast(t("chat.interrupt.pending"));
        return false;
      }
      if (isCompacting || compactingRef.current) {
        showTransientToast(t("chat.compactInProgress"), { tone: "warning" });
        return false;
      }
      if (sessionReadOnly) {
        showTransientToast(
          sessionEndReason === "compacted" || !sessionEndReason
            ? t("chat.sessionCompactedReadOnly")
            : t("chat.sessionEndedReadOnly"),
          { tone: "warning" },
        );
        return false;
      }
      if (
        (!text && pending.length === 0 && !opts?.allowEmpty && !resumeJson) ||
        streaming ||
        turnInFlightRef.current ||
        sendStartLockRef.current ||
        !activeProvider
      ) {
        return false;
      }

      // resolve /skill and @mentions
      let displayText = text;
      let modelBody = text;
      sendStartLockRef.current = true;
      try {
        if (text && !resumeJson) {
          try {
            const skillList = await invoke<
              { id: string; name: string; enabled?: boolean }[]
            >("list_installed_skills").catch(() => []);
            const mcpList = await invoke<
              { id: string; name: string; enabled?: boolean }[]
            >("get_mcp_servers").catch(() => []);

            const resolved = await resolveComposerTurn(text, {
              agents: [],
              skills: (skillList ?? [])
                .filter((s) => s.enabled !== false)
                .map((s) => ({ id: s.id, name: s.name })),
              mcpServers: (mcpList ?? []).map((s) => ({
                id: s.id,
                name: s.name,
              })),
            });

            if (resolved === null) {
              return false;
            }

            displayText = resolved.displayText || text;
            modelBody = resolved.modelText || text;

            if (resolved.enableMcpIds.length > 0) {
              try {
                const servers =
                  await invoke<
                    {
                      id: string;
                      name: string;
                      enabled: boolean;
                      [k: string]: unknown;
                    }[]
                  >("get_mcp_servers");
                const want = new Set(resolved.enableMcpIds);
                const next = (servers ?? []).map((s) =>
                  want.has(s.id) ? { ...s, enabled: true } : s,
                );
                await invoke("set_mcp_servers", { servers: next });
                showTransientToast(
                  t("chat.mentionMcpEnabled", {
                    names: resolved.enableMcpNames.join(", "),
                  }),
                );
              } catch (e) {
                console.warn("enable mcp failed", e);
              }
            }

            if (resolved.loadedSkills.length > 0) {
              showTransientToast(
                t("chat.skillLoaded", {
                  names: resolved.loadedSkills.join(", "),
                }),
              );
            }
          } catch (e) {
            console.warn("resolveComposerTurn failed", e);
          }
        }
      } finally {
        sendStartLockRef.current = false;
      }

      const isCreatingAgent = emptyMode === "agent" && !opts?.skipUserAppend;
      const userId = opts?.reuseUserId ?? `u-${Date.now()}`;
      const assistantId = `a-${Date.now()}`;
      const sid = sessionId ?? crypto.randomUUID();
      setSessionId(sid);

      setMessages((prev) => {
        const base =
          opts?.truncateTo != null ? prev.slice(0, opts.truncateTo) : prev;
        const next = [...base];
        if (!opts?.skipUserAppend) {
          next.push({
            id: userId,
            role: "user",
            content: displayText,
            attachments: pending.map((a) => ({ ...a })),
            createdAt: Date.now(),
          });
        }
        next.push({
          id: assistantId,
          role: "assistant",
          content: "",
          activities: [],
          turnStatus: "running",
          createdAt: Date.now(),
          generationStartedAt: Date.now(),
        });
        return next;
      });
      setEmptyMode(null);
      if (!opts?.skipUserAppend) {
        setInput("");
        setAttachments([]);
      }
      setStreaming(true);
      turnInFlightRef.current = true;
      setTurnInFlight(true);
      touchActivity();
      setStreamPaused(false);
      setTokenUsage(null);
      setContextUsage(null);
      setStatus("busy");
      setStatusPhase("connecting");
      setStatusDetail(null);
      clearStreamBuffers();
      generatingPreviewApi.reset();
      activeAssistantIdRef.current = assistantId;
      streamStartRef.current.set(assistantId, Date.now());
      firstTokenRef.current.delete(assistantId);
      pendingUsageRef.current.delete(assistantId);

      const effectiveMode = opts?.interactionMode ?? chatMode;
      // 交互模式说明由后端写入 system prompt，不拼进用户消息（避免污染历史/UI）
      const contentForModel = isCreatingAgent
        ? `${modelBody}\n\n---\n${t("chat.agentCreateHint")}`
        : modelBody;

      try {
        unlistenRef.current?.();

        const eventName = `chat_stream_${sid}`;
        const gen = ++streamGenRef.current;
        let terminalOutcome: string | null = null;
        let terminalError: string | null = null;
        let hasTextOutput = false;
        let hasStructuredOutput = false;
        let completionSettled = false;
        toolDeltaIdsRef.current.clear();
        unlistenRef.current = await listen<{
          type: string;
          content?: string;
          questions?: Array<{ title: string; options?: string[] }>;
          message?: string;
          id?: string;
          name?: string;
          arguments_json?: string;
          arguments?: string;
          result?: string;
          web_action?: ChatActivity["webAction"];
          web_page_title?: string;
          delta?: string;
          phase?: string;
          batch_id?: string;
          execution_mode?: "serial" | "parallel";
          media?: Array<{
            kind?: string;
            mime_type?: string;
            ref_kind?: string;
            ref_value?: string;
            label?: string;
            id?: string;
          }>;
          file_changes?: ChatActivity["fileChanges"];
          operation?: string;
          detail?: string;
          outcome?: string;
          index?: number;
          prompt_tokens?: number;
          uncached_input_tokens?: number;
          completion_tokens?: number;
          total_tokens?: number;
          provider_total_tokens?: number;
          cache_read_tokens?: number;
          cache_write_tokens?: number;
          reasoning_tokens?: number;
          request_count?: number;
          cache_read_reported?: boolean;
          cache_write_reported?: boolean;
          reasoning_reported?: boolean;
          context_window?: number;
          estimated_total_tokens?: number;
          source?: string;
          latest_usage?: {
            input_tokens?: number;
            uncached_input_tokens?: number;
            output_tokens?: number;
            total_tokens?: number;
            provider_total_tokens?: number;
            cache_read_tokens?: number;
            cache_write_tokens?: number;
            reasoning_tokens?: number;
            cache_read_reported?: boolean;
            cache_write_reported?: boolean;
            reasoning_reported?: boolean;
          } | null;
          segments?: Array<{
            id: string;
            tokens: number;
            count?: number | null;
          }>;
          updated_at?: number;
          thread_id?: string;
          run_id?: string;
          message_id?: string;
          activity_type?: string;
          content_json?: string;
          replace?: boolean;
          outcome_type?: string;
          interrupts_json?: string;
          citations?: string;
          client_message_id?: string;
        }>(eventName, (event) => {
          if (streamGenRef.current !== gen) return;
          const payload = event.payload;

          if (payload.type === "token" && payload.content) {
            if (payload.content.trim()) hasTextOutput = true;
            if (toolDeltaRafRef.current != null) {
              cancelAnimationFrame(toolDeltaRafRef.current);
              flushToolDeltas();
            }
            touchActivity();
            enqueueStreamToken(assistantId, payload.content);
          } else if (
            payload.type === "async_message" &&
            payload.id &&
            payload.content
          ) {
            setMessages((prev) =>
              upsertAsyncAgentUpdate(
                prev,
                assistantId,
                payload.id!,
                payload.content!,
                payload.questions,
              ),
            );
            touchActivity();
          } else if (payload.type === "text_reconcile") {
            if (toolDeltaRafRef.current != null) {
              cancelAnimationFrame(toolDeltaRafRef.current);
              flushToolDeltas();
            }
            if (streamRafRef.current != null) {
              cancelAnimationFrame(streamRafRef.current);
            }
            flushStreamTokens();
            const canonical = payload.content ?? "";
            hasTextOutput = canonical.trim().length > 0;
            setMessages((prev) =>
              prev.map((message) =>
                message.id === assistantId
                  ? reconcileText(message, canonical)
                  : message,
              ),
            );
            touchActivity();
          } else if (payload.type === "reasoning" && payload.content) {
            if (toolDeltaRafRef.current != null) {
              cancelAnimationFrame(toolDeltaRafRef.current);
              flushToolDeltas();
            }
            touchActivity();
            enqueueStreamReasoning(assistantId, payload.content);
            setStatusPhase("generating");
          } else if (payload.type === "reasoning_reconcile") {
            if (toolDeltaRafRef.current != null) {
              cancelAnimationFrame(toolDeltaRafRef.current);
              flushToolDeltas();
            }
            if (streamRafRef.current != null) {
              cancelAnimationFrame(streamRafRef.current);
            }
            flushStreamTokens();
            const canonical = payload.content ?? "";
            setMessages((prev) =>
              prev.map((message) =>
                message.id === assistantId
                  ? reconcileReasoning(message, canonical)
                  : message,
              ),
            );
            touchActivity();
            setStatusPhase("generating");
          } else if (payload.type === "citations" && payload.citations) {
            try {
              const parsed = JSON.parse(payload.citations) as Array<
                Record<string, unknown>
              >;
              setMessages((prev) =>
                prev.map((m) =>
                  m.id === assistantId
                    ? { ...m, citations: [...(m.citations ?? []), ...parsed] }
                    : m,
                ),
              );
            } catch {}
          } else if (payload.type === "usage") {
            const usage: TurnTokenUsage = {
              promptTokens: payload.prompt_tokens ?? 0,
              uncachedInputTokens:
                payload.uncached_input_tokens ?? payload.prompt_tokens ?? 0,
              completionTokens: payload.completion_tokens ?? 0,
              totalTokens: payload.total_tokens ?? 0,
              providerTotalTokens: payload.provider_total_tokens,
              cacheReadTokens: payload.cache_read_tokens ?? 0,
              cacheWriteTokens: payload.cache_write_tokens ?? 0,
              reasoningTokens: payload.reasoning_tokens ?? 0,
              requestCount: payload.request_count ?? 0,
              cacheReadReported: payload.cache_read_reported === true,
              cacheWriteReported: payload.cache_write_reported === true,
              reasoningReported: payload.reasoning_reported === true,
            };
            pendingUsageRef.current.set(assistantId, usage);
            setTokenUsage(usage);
            setMessages((prev) =>
              prev.map((m) => (m.id === assistantId ? { ...m, usage } : m)),
            );
          } else if (payload.type === "context_usage") {
            const snap = normalizeContextUsageEvent(payload);
            setContextUsage(snap);
            if (sid && persistContextUsage)
              saveContextUsageForSession(sid, snap);
            if (snap.recommendCompact) {
              const now = Date.now();
              // 流式中只提示，不自动拆 session；60s 冷却避免刷屏
              if (now - lastRecommendCompactToastAtRef.current >= 60_000) {
                lastRecommendCompactToastAtRef.current = now;
                showTransientToast(t("chat.recommendCompact"), {
                  tone: "warning",
                });
              }
            }
          } else if (payload.type === "run_started") {
            const runId = payload.run_id ?? null;
            currentRunIdRef.current = runId;
            setCurrentTurnId(runId);
            setMessages((prev) =>
              prev.map((message) =>
                message.id === assistantId
                  ? { ...message, turnStatus: "running" }
                  : message,
              ),
            );
          } else if (
            payload.type === "user_input_committed" &&
            payload.client_message_id
          ) {
            onUserInputCommitted?.(payload.client_message_id);
          } else if (payload.type === "activity") {
            hasStructuredOutput = true;
            if (streamRafRef.current != null) {
              cancelAnimationFrame(streamRafRef.current);
              flushStreamTokens();
            }
            if (toolDeltaRafRef.current != null) {
              cancelAnimationFrame(toolDeltaRafRef.current);
              flushToolDeltas();
            }
            const operations = parseActivityOperations(payload.content_json);
            const surface: UiSurface = {
              messageId: payload.message_id || `surf-${Date.now()}`,
              activityType: payload.activity_type || "a2ui-surface",
              operations,
              status: "active",
            };
            setMessages((prev) =>
              prev.map((m) => {
                if (m.id !== assistantId) return m;
                return applySurfaceUpsert(m, surface);
              }),
            );
            setStatusPhase("generating");
          } else if (payload.type === "run_finished") {
            terminalOutcome = payload.outcome_type ?? null;
            if (
              payload.outcome_type === "hitl_waiting" ||
              payload.outcome_type === "interrupt"
            ) {
              const { interrupts, surface } = parseHitlRunFinished(
                payload.interrupts_json,
                assistantId,
              );
              const waitingToolIds = new Set(
                interrupts
                  .map((interrupt) => interrupt.toolCallId)
                  .filter((id): id is string => Boolean(id)),
              );
              setSessionPendingInterrupts(interrupts);
              setMessages((prev) =>
                prev.map((m) => {
                  if (m.id !== assistantId) return m;
                  let next: ConversationEntry = {
                    ...m,
                    turnStatus:
                      payload.outcome_type === "interrupt"
                        ? ("interrupted" as const)
                        : ("waiting" as const),
                    activities: m.activities?.map((activity) => {
                      if (
                        payload.outcome_type === "hitl_waiting" &&
                        waitingToolIds.has(activity.id)
                      ) {
                        return { ...activity, status: "waiting" as const };
                      }
                      if (
                        payload.outcome_type === "interrupt" &&
                        isLiveActivityStatus(activity.status)
                      ) {
                        return { ...activity, status: "interrupted" as const };
                      }
                      return activity;
                    }),
                  };
                  if (surface) {
                    next = applySurfaceUpsert(next, surface);
                  } else {
                    const surfaces = [...(m.uiSurfaces ?? [])];
                    const last = surfaces[surfaces.length - 1]!;
                    if (last) {
                      next = applySurfaceUpsert(next, {
                        ...last,
                        interrupts: interrupts.map(
                          ({ id, reason, message, responseSchema }) => ({
                            id,
                            reason,
                            message,
                            responseSchema,
                          }),
                        ),
                      });
                    }
                  }
                  return sealOpenReasoning(next, Date.now());
                }),
              );
              if (payload.outcome_type === "interrupt") {
                setStreaming(false);
                setStreamPaused(false);
                setStatus("ready");
                setStatusPhase("ready");
              } else {
                setStatusPhase("generating");
              }
            } else if (payload.outcome_type === "success") {
              setSessionPendingInterrupts([]);
            } else if (payload.outcome_type === "error") {
              terminalError ??= t("status.unknownError");
              setStatus("error");
              setStatusPhase("error");
            }
          } else if (payload.type === "tool_call_delta") {
            if (streamRafRef.current != null) {
              cancelAnimationFrame(streamRafRef.current);
              flushStreamTokens();
            }
            touchActivity();
            enqueueToolDelta(assistantId, {
              index: payload.index ?? 0,
              id: payload.id,
              name: payload.name,
              arguments: payload.arguments,
            });
            generatingPreviewApi.onToolDelta({
              index: payload.index ?? 0,
              id: payload.id,
              name: payload.name,
              arguments: payload.arguments,
            });
            setStatusPhase("generating");
          } else if (payload.type === "tool_call") {
            hasStructuredOutput = true;
            touchActivity();
            if (streamRafRef.current != null) {
              cancelAnimationFrame(streamRafRef.current);
              flushStreamTokens();
            }
            if (toolDeltaRafRef.current != null) {
              cancelAnimationFrame(toolDeltaRafRef.current);
              flushToolDeltas();
            }
            const name = payload.name ?? "tool";
            const lower = name.toLowerCase();
            const kind: ChatActivity["kind"] = lower.startsWith("mcp_")
              ? "mcp"
              : lower.startsWith("skill_") || lower.includes("skill")
                ? "skill"
                : lower.includes("hook")
                  ? "hook"
                  : "tool";
            const id = payload.id || `act-${Date.now()}`;
            const structuredMedia = Array.isArray(payload.media)
              ? (payload.media
                  .map((m) => {
                    const path = m.ref_value;
                    let kind:
                      | NonNullable<ChatActivity["media"]>[number]["kind"]
                      | null =
                      m.kind === "image" ||
                      m.kind === "video" ||
                      m.kind === "audio" ||
                      m.kind === "html"
                        ? m.kind
                        : null;
                    // 后端把写工具产物标为 kind="file"；按扩展名归类（对齐历史加载逻辑）
                    if (!kind && m.kind === "file" && path) {
                      if (/\.html?$/i.test(path)) kind = "html";
                      else if (
                        /\.(png|jpe?g|webp|gif|bmp|svg|avif)$/i.test(path)
                      )
                        kind = "image";
                      else if (/\.(mp4|webm|mov|mkv|m4v)$/i.test(path))
                        kind = "video";
                      else if (/\.(wav|mp3|m4a|aac|ogg|flac|opus)$/i.test(path))
                        kind = "audio";
                      else if (isCodePath(path)) kind = "code";
                    }
                    if (!kind || !path) return null;
                    return { kind, path };
                  })
                  .filter(Boolean) as ChatActivity["media"])
              : undefined;
            generatingPreviewApi.onToolCall({
              id: payload.id,
              name: payload.name,
              arguments_json: payload.arguments_json,
              result: payload.result,
            });
            browserPreviewApi.onToolCall({
              name: payload.name,
              arguments_json: payload.arguments_json,
              result: payload.result,
              phase: payload.phase,
            });
            if (!pendingModeSwitch && payload.result) {
              const sw = parseModeSwitchResult(payload.result);
              if (sw) {
                pendingModeSwitch = sw;
                onModeSwitchDetected?.(sw);
              }
            }
            const activity: ChatActivity = {
              id,
              kind,
              title: name,
              input: payload.arguments_json || undefined,
              output: payload.result || undefined,
              webAction: normalizeChatWebAction(payload.web_action),
              webPageTitle: payload.web_page_title?.trim() || undefined,
              detail: payload.result || payload.arguments_json || undefined,
              status: resolveToolActivityStatus(payload.phase, payload.result),
              at: Date.now(),
              batchId: payload.batch_id,
              executionMode: payload.execution_mode,
              media: structuredMedia,
              fileChanges: payload.file_changes,
            };
            setMessages((prev) =>
              prev.map((m) => {
                if (m.id !== assistantId) return m;
                const activities = m.activities ?? [];
                let merged = activity;
                let idx = activities.findIndex((a) => a.id === activity.id);
                if (idx < 0) {
                  idx = activities.findIndex(
                    (a) =>
                      a.status === "running" &&
                      (a.title === name || a.title.startsWith("tool#")),
                  );
                }
                if (idx >= 0) {
                  merged = {
                    ...activities[idx]!,
                    ...activity,
                    id: activities[idx]!.id,
                    at: activities[idx]!.at ?? activity.at,
                  };
                }
                return applyActivityUpsert(m, merged);
              }),
            );
          } else if (
            payload.type === "tool_output_delta" &&
            payload.id &&
            payload.delta
          ) {
            touchActivity();
            const { id, delta } = payload as { id: string; delta: string };
            setMessages((prev) =>
              prev.map((m) => {
                if (m.id !== assistantId) return m;
                // 增量只补已经开卡的工具调用；先于 tool_call started 到达时丢弃，
                // 完成事件仍会带上完整输出。
                const existing = (m.activities ?? []).find((a) => a.id === id);
                if (!existing || isSettledActivityStatus(existing.status))
                  return m;
                const output = `${existing.output ?? ""}${delta}`;
                return applyActivityUpsert(m, {
                  ...existing,
                  output,
                  detail: output,
                  status: "running",
                });
              }),
            );
            setStatusPhase("generating");
          } else if (payload.type === "memory_update") {
            hasStructuredOutput = true;
            const activity: ChatActivity = {
              id: `mem-${Date.now()}`,
              kind: "memory",
              title: payload.operation || "memory",
              output: payload.content,
              detail: payload.content,
              status: "done",
              at: Date.now(),
            };
            setMessages((prev) =>
              prev.map((m) =>
                m.id === assistantId ? applyActivityUpsert(m, activity) : m,
              ),
            );
            if (
              chatDisplayPrefsRef.current?.showMemory &&
              payload.operation === "background_review" &&
              payload.content
            ) {
              showTransientToast(payload.content);
            }
          } else if (payload.type === "hook") {
            hasStructuredOutput = true;
            const title = payload.name || "hook";
            const detail = [payload.detail, payload.outcome]
              .filter((s) => typeof s === "string" && s.trim())
              .join(" · ");
            const activity: ChatActivity = {
              id: `hook-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`,
              kind: "hook",
              title,
              output: detail || title,
              detail: detail || title,
              status: "done",
              at: Date.now(),
            };
            setMessages((prev) =>
              prev.map((m) =>
                m.id === assistantId ? applyActivityUpsert(m, activity) : m,
              ),
            );
          } else if (payload.type === "done") {
            if (completionSettled || terminalOutcome === "hitl_waiting") return;
            completionSettled = true;
            const completion = resolveTaskCompletion({
              outcome: terminalOutcome,
              terminalError,
              hasRenderableOutput: hasTextOutput || hasStructuredOutput,
              emptyResponseError: t("status.emptyResponse"),
            });
            const runFailed = completion.failed;
            if (streamRafRef.current != null) {
              cancelAnimationFrame(streamRafRef.current);
              flushStreamTokens();
            }
            if (toolDeltaRafRef.current != null) {
              cancelAnimationFrame(toolDeltaRafRef.current);
              flushToolDeltas();
            }
            setMessages((prev) => {
              const endedAt = Date.now();
              const usage = pendingUsageRef.current.get(assistantId);
              const genStart =
                firstTokenRef.current.get(assistantId) ??
                streamStartRef.current.get(assistantId);
              const tokensPerSec =
                usage && genStart != null
                  ? calcTokensPerSec(usage.completionTokens, endedAt - genStart)
                  : undefined;
              const next = prev.map((m) => {
                if (m.id !== assistantId) return m;
                const pendingText =
                  streamPendingRef.current.get(assistantId) ?? "";
                const content = (m.content + pendingText).trim();
                const withUsage = sealOpenReasoning(
                  {
                    ...m,
                    turnStatus: completion.failed
                      ? ("error" as const)
                      : terminalOutcome === "interrupt"
                        ? ("interrupted" as const)
                        : ("done" as const),
                    usage: usage ?? m.usage,
                    tokensPerSec: tokensPerSec ?? m.tokensPerSec,
                    generationDurationSec:
                      m.generationDurationSec ??
                      (m.generationStartedAt != null
                        ? elapsedSecSince(m.generationStartedAt, endedAt)
                        : streamStartRef.current.has(assistantId)
                          ? elapsedSecSince(
                              streamStartRef.current.get(assistantId)!,
                              endedAt,
                            )
                          : undefined),
                    generationStartedAt: undefined,
                  },
                  endedAt,
                );
                if (
                  !content &&
                  !(m.activities && m.activities.length > 0) &&
                  !(m.uiSurfaces && m.uiSurfaces.length > 0) &&
                  !(m.attachments && m.attachments.length > 0)
                ) {
                  return {
                    ...withUsage,
                    content:
                      !completion.failed && terminalOutcome === "interrupt"
                        ? t("chat.task.cancelled")
                        : (completion.error ?? t("status.emptyResponse")),
                    error: completion.failed,
                  };
                }
                return withUsage;
              });
              streamPendingRef.current.delete(assistantId);
              streamStartRef.current.delete(assistantId);
              firstTokenRef.current.delete(assistantId);
              pendingUsageRef.current.delete(assistantId);
              activeAssistantIdRef.current = null;
              return next;
            });
            generatingPreviewApi.onStreamEnd();
            setStreaming(false);
            setStreamPaused(false);
            markTurnEnded();
            setStatus(runFailed ? "error" : "ready");
            setStatusPhase(runFailed ? "error" : "ready");
            setStatusDetail(runFailed ? completion.error : null);
            if (completion.celebrate) {
              onTurnSucceeded?.();
            }
            if (pendingModeSwitch) {
              onModeSwitchPrompt?.(pendingModeSwitch);
            }
          } else if (payload.type === "error") {
            if (streamRafRef.current != null) {
              cancelAnimationFrame(streamRafRef.current);
              flushStreamTokens();
            }
            if (toolDeltaRafRef.current != null) {
              cancelAnimationFrame(toolDeltaRafRef.current);
              flushToolDeltas();
            }
            const errMsg = payload.message || t("status.unknownError");
            terminalError = errMsg;
            setMessages((prev) =>
              prev.map((m) => {
                if (m.id !== assistantId) return m;
                const base = (m.content ?? "").trim();
                return sealOpenReasoning(
                  {
                    ...m,
                    turnStatus: "error" as const,
                    content: base ? `${base}\n\n⚠️ ${errMsg}` : errMsg,
                    error: true,
                    generationDurationSec:
                      m.generationDurationSec ??
                      (m.generationStartedAt != null
                        ? elapsedSecSince(m.generationStartedAt)
                        : undefined),
                    generationStartedAt: undefined,
                  },
                  Date.now(),
                );
              }),
            );
            setStatus("error");
            setStatusPhase("error");
            setStatusDetail(errMsg);
          }
        });

        const uploadedPaths = new Map<string, string>();
        await Promise.all(
          pending
            .filter((a) => !!a.dataBase64)
            .map(async (a) => {
              try {
                const saved = await invoke<{ path: string }>(
                  "save_chat_upload",
                  {
                    sessionId: sid,
                    fileName: a.name,
                    dataBase64: a.dataBase64,
                    messageId: userId,
                  },
                );
                uploadedPaths.set(a.id, saved.path);
              } catch (e) {
                console.warn("save_chat_upload failed", e);
              }
            }),
        );

        const globals = loadPickerGlobals();
        let chatProvider: ProviderDto = activeProvider;
        let chatModel = activeProvider.model;

        if (globals.auto) {
          try {
            const candidates = await loadModelCandidates(providers);
            const picked = selectAutoModel({
              text,
              hasImages: pending.some((a) => a.kind === "image"),
              chatMode,
              maxMode: globals.maxMode,
              candidates,
              preferProviderId: activeProvider.id,
            });
            if (picked) {
              const p = providers.find((x) => x.id === picked.providerId);
              if (p) {
                chatProvider = p;
                chatModel = picked.modelId;
              }
            }
          } catch (e) {
            console.warn("auto model select failed", e);
          }
        }

        const sendSupportsThinking = shouldShowThinkingControls({
          capabilities: null,
          backendId: chatProvider.backend_id,
        });
        const modelApi = sendSupportsThinking
          ? modelPrefsToApi(loadModelPrefs(chatProvider.id, chatModel), globals)
          : { thinkingEnabled: false, reasoningEffort: "high" as const };

        const keepChatBubbles = pendingKeepChatBubblesRef.current;

        await invoke<string>("start_chat", {
          request: {
            content: contentForModel,
            provider: chatProvider.backend_id,
            providerId: chatProvider.id,
            model: chatModel,
            sessionId: sid,
            useMemory: true,
            thinkingEnabled: modelApi.thinkingEnabled,
            reasoningEffort: modelApi.reasoningEffort,
            resumeJson: resumeJson || undefined,
            keepChatBubbles:
              keepChatBubbles != null ? keepChatBubbles : undefined,
            interactionMode: effectiveMode,
            projectId,
            attachments: pending.map((a) => ({
              name: a.name,
              mime: a.mime,
              kind: a.kind,
              size: a.size,
              dataBase64: a.dataBase64 ?? null,
              localPath: uploadedPaths.get(a.id) ?? a.localPath ?? null,
            })),
          },
        });
        pendingKeepChatBubblesRef.current = null;
        setStatusPhase("generating");
      } catch (err) {
        pendingKeepChatBubblesRef.current = null;
        clearStreamBuffers();
        setMessages((prev) =>
          prev.map((m) =>
            m.id === assistantId
              ? {
                  ...m,
                  content: String(err),
                  error: true,
                  turnStatus: "error",
                }
              : m,
          ),
        );
        setStreaming(false);
        setStreamPaused(false);
        markTurnEnded();
        setStatus("error");
        setStatusPhase("error");
        setStatusDetail(null);
      }
      return true;
    },
    // depsRef always holds the latest values, so send never needs to be recreated
    [],
  );

  return { send };
}
