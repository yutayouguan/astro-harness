import { useCallback, useRef } from "react";
import type { Dispatch, MutableRefObject, RefObject, SetStateAction } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  applyActivityUpsert,
  applySurfaceUpsert,
  sealOpenReasoning,
} from "../../lib/chat/chatTimeline";
import { elapsedSecSince } from "../../lib/chat/elapsedSec";
import { normalizeContextUsageEvent } from "../../lib/chat/contextUsage";
import { saveContextUsageForSession } from "../../lib/chat/chatSessionStore";
import {
  parseModeSwitchResult,
  type ChatInteractionMode,
  type ModeSwitchRequest,
} from "../../lib/chat/chatMode";
import { resolveComposerTurn } from "../../lib/chat/composerResolve";
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
  ChatMessage,
  MessageTokenUsage,
  PendingInterrupt,
  ProviderDto,
  UiSurface,
} from "../../types";
import type { ContextUsageSnapshot } from "../../lib/chat/contextUsage";
import type { GeneratingPreviewApi } from "./useGeneratingPreview";
import { isCodePath } from "../../lib/media/parseGeneratedMedia";
import type { ChatDisplayPrefs } from "./useChatDisplayPrefs";
import type { ShowToastOptions } from "../ui/useTransientToast";
import type { MessageKey } from "../../i18n/messages";
import { useActiveAgent } from "../app/useActiveAgent";

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
  messages: ChatMessage[];
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
  // stream buffer refs
  streamGenRef: MutableRefObject<number>;
  currentRunIdRef: MutableRefObject<string | null>;
  activeAssistantIdRef: MutableRefObject<string | null>;
  streamStartRef: MutableRefObject<Map<string, number>>;
  firstTokenRef: MutableRefObject<Map<string, number>>;
  pendingUsageRef: MutableRefObject<Map<string, MessageTokenUsage>>;
  streamPendingRef: MutableRefObject<Map<string, string>>;
  toolDeltaIdsRef: MutableRefObject<Map<string, string>>;
  toolDeltaRafRef: MutableRefObject<number | null>;
  streamRafRef: MutableRefObject<number | null>;
  // session refs
  unlistenRef: MutableRefObject<UnlistenFn | null>;
  compactingRef: MutableRefObject<boolean>;
  pendingKeepChatBubblesRef: MutableRefObject<number | null>;
  dissolvingIdsRef: MutableRefObject<string[]>;
  dissolveTimerRef: MutableRefObject<number | null>;
  /** recommendCompact toast 冷却（ms epoch） */
  lastRecommendCompactToastAtRef: MutableRefObject<number>;
  // setters
  setMessages: Dispatch<SetStateAction<ChatMessage[]>>;
  setSessionId: Dispatch<SetStateAction<string | null>>;
  setStreaming: Dispatch<SetStateAction<boolean>>;
  setStreamPaused: Dispatch<SetStateAction<boolean>>;
  setTokenUsage: Dispatch<SetStateAction<MessageTokenUsage | null>>;
  setContextUsage: Dispatch<SetStateAction<ContextUsageSnapshot | null>>;
  setStatus: Dispatch<SetStateAction<"ready" | "busy" | "error">>;
  setStatusPhase: Dispatch<SetStateAction<StatusPhase>>;
  setStatusDetail: Dispatch<SetStateAction<string | null>>;
  setEmptyMode: Dispatch<SetStateAction<ChatEmptyMode>>;
  setInput: Dispatch<SetStateAction<string>>;
  setAttachments: Dispatch<SetStateAction<ChatAttachment[]>>;
  setSessionPendingInterrupts: Dispatch<SetStateAction<PendingInterrupt[]>>;
  setCurrentTurnId: Dispatch<SetStateAction<string | null>>;
  setDissolvingIds: Dispatch<SetStateAction<string[]>>;
  showTransientToast: ShowToastFn;
  /** 主会话整轮未结束（含 HITL 停顿）；供队列软边界 */
  turnInFlightRef: MutableRefObject<boolean>;
  setTurnInFlight: Dispatch<SetStateAction<boolean>>;
  /** Agent 会话级 worktree（按 sessionId 复用） */
  sessionWorktreeRef: MutableRefObject<{
    sessionId: string;
    path: string;
    repoRoot: string;
    branch: string;
  } | null>;
  /** 本轮首次检测到模式切换请求时记录（流结束后再弹授权条） */
  onModeSwitchDetected?: (req: ModeSwitchRequest) => void;
  /** 流正常结束后，若本轮有模式切换请求则提示 UI */
  onModeSwitchPrompt?: (req: ModeSwitchRequest) => void;
  /** A steered queue item is durable in the active turn history. */
  onUserInputCommitted?: (clientMessageId: string) => void;
}

function calcTokensPerSec(completionTokens: number, durationMs: number): number | undefined {
  if (completionTokens <= 0 || durationMs <= 0) return undefined;
  const sec = Math.max(0.1, durationMs / 1000);
  return Math.round((completionTokens / sec) * 10) / 10;
}

export function useSend(deps: UseSendDeps) {
  const depsRef = useRef(deps);
  depsRef.current = deps;
  const { setActiveAgent } = useActiveAgent();
  const setActiveAgentRef = useRef(setActiveAgent);
  setActiveAgentRef.current = setActiveAgent;

  /** @returns 是否已开始流式（`setStreaming(true)` 之后）；供 follow-up 队列判断是否出队成功 */
  const send = useCallback(
    async (opts?: SendOpts): Promise<boolean> => {
      const {
        input,
        attachments,
        streaming,
        isCompacting,
        sessionReadOnly,
        sessionEndReason,
        activeProvider,
        providers,
        sessionId,
        messages,
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
        dissolvingIdsRef,
        dissolveTimerRef,
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
        setDissolvingIds,
        showTransientToast,
        turnInFlightRef,
        setTurnInFlight,
        sessionWorktreeRef,
        onModeSwitchDetected,
        onModeSwitchPrompt,
        onUserInputCommitted,
      } = depsRef.current;

      const markTurnEnded = () => {
        turnInFlightRef.current = false;
        setTurnInFlight(false);
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
        !activeProvider
      ) {
        return false;
      }

      // flush any pending dissolve before sending
      if (dissolveTimerRef.current != null) {
        window.clearTimeout(dissolveTimerRef.current);
        dissolveTimerRef.current = null;
      }
      if (dissolvingIdsRef.current.length > 0) {
        const cutId = dissolvingIdsRef.current[0];
        setDissolvingIds([]);
        setMessages((prev) => {
          const cut = prev.findIndex((m) => m.id === cutId);
          return cut < 0 ? prev : prev.slice(0, cut);
        });
      }

      // resolve /skill and @mentions
      let displayText = text;
      let modelBody = text;
      if (text && !resumeJson) {
        try {
          const cfg = await invoke<{
            agents: { id: string; name: string }[];
            active_agent_id?: string;
          }>("get_config");
          const skillList = await invoke<
            { id: string; name: string; enabled?: boolean }[]
          >("list_installed_skills").catch(() => []);
          const mcpList = await invoke<
            { id: string; name: string; enabled?: boolean }[]
          >("get_mcp_servers", { agentId: null }).catch(() => []);

          const resolved = await resolveComposerTurn(text, {
            agents: cfg.agents ?? [],
            skills: (skillList ?? [])
              .filter((s) => s.enabled !== false)
              .map((s) => ({ id: s.id, name: s.name })),
            mcpServers: (mcpList ?? []).map((s) => ({ id: s.id, name: s.name })),
          });

          if (resolved === null) {
            return false;
          }

          displayText = resolved.displayText || text;
          modelBody = resolved.modelText || text;

          if (resolved.switchAgentId) {
            try {
              await setActiveAgentRef.current(resolved.switchAgentId);
              const hasHistory = messages.some(
                (m) => m.id !== "welcome" && (m.role === "user" || m.role === "assistant"),
              );
              showTransientToast(
                t(
                  hasHistory
                    ? "chat.mentionAgentSwitchedLater"
                    : "chat.mentionAgentSwitched",
                  { name: resolved.switchAgentName ?? resolved.switchAgentId },
                ),
              );
            } catch (e) {
              console.warn("set_active_agent failed", e);
            }
          }

          if (resolved.enableMcpIds.length > 0) {
            try {
              const servers = await invoke<
                { id: string; name: string; enabled: boolean; [k: string]: unknown }[]
              >("get_mcp_servers", { agentId: null });
              const want = new Set(resolved.enableMcpIds);
              const next = (servers ?? []).map((s) =>
                want.has(s.id) ? { ...s, enabled: true } : s,
              );
              await invoke("set_mcp_servers", { servers: next, agentId: null });
              showTransientToast(
                t("chat.mentionMcpEnabled", { names: resolved.enableMcpNames.join(", ") }),
              );
            } catch (e) {
              console.warn("enable mcp failed", e);
            }
          }

          if (resolved.loadedSkills.length > 0) {
            showTransientToast(
              t("chat.skillLoaded", { names: resolved.loadedSkills.join(", ") }),
            );
          }
        } catch (e) {
          console.warn("resolveComposerTurn failed", e);
        }
      }

      const isCreatingAgent = emptyMode === "agent" && !opts?.skipUserAppend;
      const userId = opts?.reuseUserId ?? `u-${Date.now()}`;
      const assistantId = `a-${Date.now()}`;
      const sid = sessionId ?? crypto.randomUUID();
      setSessionId(sid);

      setMessages((prev) => {
        const base = opts?.truncateTo != null ? prev.slice(0, opts.truncateTo) : prev;
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

        const eventName = `chat-stream-${sid}`;
        const gen = ++streamGenRef.current;
        let terminalOutcome: string | null = null;
        let terminalError: string | null = null;
        toolDeltaIdsRef.current.clear();
        unlistenRef.current = await listen<{
          type: string;
          content?: string;
          message?: string;
          id?: string;
          name?: string;
          arguments_json?: string;
          arguments?: string;
          result?: string;
          phase?: string;
          media?: Array<{
            kind?: string;
            mime_type?: string;
            ref_kind?: string;
            ref_value?: string;
            label?: string;
            id?: string;
          }>;
          operation?: string;
          detail?: string;
          outcome?: string;
          index?: number;
          prompt_tokens?: number;
          completion_tokens?: number;
          total_tokens?: number;
          context_window?: number;
          segments?: Array<{ id: string; tokens: number; count?: number | null }>;
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
            enqueueStreamToken(assistantId, payload.content);
          } else if (payload.type === "reasoning" && payload.content) {
            enqueueStreamReasoning(assistantId, payload.content);
            setStatusPhase("generating");
          } else if (payload.type === "citations" && payload.citations) {
            try {
              const parsed = JSON.parse(payload.citations) as Array<Record<string, unknown>>;
              setMessages((prev) =>
                prev.map((m) =>
                  m.id === assistantId
                    ? { ...m, citations: [...(m.citations ?? []), ...parsed] }
                    : m,
                ),
              );
            } catch {}
          } else if (payload.type === "usage") {
            const usage: MessageTokenUsage = {
              promptTokens: payload.prompt_tokens ?? 0,
              completionTokens: payload.completion_tokens ?? 0,
              totalTokens: payload.total_tokens ?? 0,
            };
            pendingUsageRef.current.set(assistantId, usage);
            setTokenUsage(usage);
            setMessages((prev) =>
              prev.map((m) => (m.id === assistantId ? { ...m, usage } : m)),
            );
          } else if (payload.type === "context_usage") {
            const snap = normalizeContextUsageEvent(payload);
            setContextUsage(snap);
            if (sid) saveContextUsageForSession(sid, snap);
            if (snap.recommendCompact) {
              const now = Date.now();
              // 流式中只提示，不自动拆 session；60s 冷却避免刷屏
              if (now - lastRecommendCompactToastAtRef.current >= 60_000) {
                lastRecommendCompactToastAtRef.current = now;
                showTransientToast(t("chat.recommendCompact"), { tone: "warning" });
              }
            }
          } else if (payload.type === "run_started") {
            const runId = payload.run_id ?? null;
            currentRunIdRef.current = runId;
            setCurrentTurnId(runId);
          } else if (
            payload.type === "user_input_committed" &&
            payload.client_message_id
          ) {
            onUserInputCommitted?.(payload.client_message_id);
          } else if (payload.type === "activity") {
            let operations: unknown[] = [];
            try {
              const parsed = JSON.parse(payload.content_json || "{}") as {
                operations?: unknown;
              };
              if (Array.isArray(parsed.operations)) {
                operations = parsed.operations;
              }
            } catch {
              /* ignore malformed activity */
            }
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
              let interrupts: PendingInterrupt[] = [];
              try {
                const arr = JSON.parse(payload.interrupts_json || "[]") as unknown;
                if (Array.isArray(arr)) {
                  interrupts = arr
                    .map((raw) => {
                      const i = raw as Record<string, unknown>;
                      let responseSchema: unknown;
                      const schemaRaw = i.response_schema_json;
                      if (typeof schemaRaw === "string" && schemaRaw.trim()) {
                        try {
                          responseSchema = JSON.parse(schemaRaw);
                        } catch {
                          responseSchema = undefined;
                        }
                      }
                      return {
                        id: String(i.id ?? ""),
                        reason: String(i.reason ?? ""),
                        message: typeof i.message === "string" ? i.message : undefined,
                        responseSchema,
                        assistantMessageId: assistantId,
                      } satisfies PendingInterrupt;
                    })
                    .filter((i) => i.id);
                }
              } catch {
                interrupts = [];
              }
              setSessionPendingInterrupts(interrupts);
              setMessages((prev) =>
                prev.map((m) => {
                  if (m.id !== assistantId) return m;
                  let next = m;
                  const surfaces = [...(m.uiSurfaces ?? [])];
                  if (surfaces.length > 0) {
                    const last = surfaces[surfaces.length - 1]!;
                    next = applySurfaceUpsert(next, {
                      ...last,
                      interrupts: interrupts.map(({ id, reason, message, responseSchema }) => ({
                        id,
                        reason,
                        message,
                        responseSchema,
                      })),
                    });
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
              ? payload.media
                  .map((m) => {
                    const path = m.ref_value;
                    let kind: NonNullable<ChatActivity["media"]>[number]["kind"] | null =
                      m.kind === "image" ||
                      m.kind === "video" ||
                      m.kind === "audio" ||
                      m.kind === "html"
                        ? m.kind
                        : null;
                    // 后端把 file_ops 产物标为 kind="file"；按扩展名归类（对齐历史加载逻辑）
                    if (!kind && m.kind === "file" && path) {
                      if (/\.html?$/i.test(path)) kind = "html";
                      else if (/\.(png|jpe?g|webp|gif|bmp|svg|avif)$/i.test(path))
                        kind = "image";
                      else if (/\.(mp4|webm|mov|mkv|m4v)$/i.test(path)) kind = "video";
                      else if (/\.(wav|mp3|m4a|aac|ogg|flac|opus)$/i.test(path))
                        kind = "audio";
                      else if (isCodePath(path)) kind = "code";
                    }
                    if (!kind || !path) return null;
                    return { kind, path };
                  })
                  .filter(Boolean) as ChatActivity["media"]
              : undefined;
            generatingPreviewApi.onToolCall({
              id: payload.id,
              name: payload.name,
              arguments_json: payload.arguments_json,
              result: payload.result,
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
              detail: payload.result || payload.arguments_json || undefined,
              status:
                payload.phase === "completed" || payload.result ? "done" : "running",
              at: Date.now(),
              media: structuredMedia,
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
          } else if (payload.type === "memory_update") {
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
            const runFailed = terminalOutcome === "error" || terminalError != null;
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
                const pendingText = streamPendingRef.current.get(assistantId) ?? "";
                const content = (m.content + pendingText).trim();
                const withUsage = sealOpenReasoning(
                  {
                    ...m,
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
                    content: t("status.emptyResponse"),
                    error: true,
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
            setStatusDetail(runFailed ? terminalError : null);
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
                const saved = await invoke<{ path: string }>("save_chat_upload", {
                  sessionId: sid,
                  fileName: a.name,
                  dataBase64: a.dataBase64,
                  messageId: userId,
                });
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

        // Agent：会话级 worktree；其它模式显式传空串，避免后端粘住上一轮 project_root
        let projectRoot = "";
        if (effectiveMode === "agent") {
          const existing = sessionWorktreeRef.current;
          if (existing && existing.sessionId === sid) {
            projectRoot = existing.path;
          } else {
            if (existing) {
              void invoke("cleanup_task_worktree", {
                path: existing.path,
                repoRoot: existing.repoRoot,
                branch: existing.branch,
              }).catch(() => {});
              sessionWorktreeRef.current = null;
            }
            try {
              const prepared = await invoke<{
                path: string;
                repoRoot: string;
                branch: string;
              } | null>("prepare_task_worktree", { taskId: sid });
              if (prepared?.path) {
                sessionWorktreeRef.current = {
                  sessionId: sid,
                  path: prepared.path,
                  repoRoot: prepared.repoRoot,
                  branch: prepared.branch,
                };
                projectRoot = prepared.path;
              }
            } catch (e) {
              console.warn("prepare session worktree failed", e);
            }
          }
        } else if (sessionWorktreeRef.current) {
          const existing = sessionWorktreeRef.current;
          void invoke("cleanup_task_worktree", {
            path: existing.path,
            repoRoot: existing.repoRoot,
            branch: existing.branch,
          }).catch(() => {});
          sessionWorktreeRef.current = null;
        }

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
            keepChatBubbles: keepChatBubbles != null ? keepChatBubbles : undefined,
            interactionMode: effectiveMode,
            projectRoot,
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
        clearStreamBuffers();
        setMessages((prev) =>
          prev.map((m) =>
            m.id === assistantId ? { ...m, content: String(err), error: true } : m,
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
