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
import {
  sealOpenReasoning,
} from "../../lib/chat/chatTimeline";
import { elapsedSecSince } from "../../lib/chat/elapsedSec";
import { type ChatInteractionMode } from "../../lib/chat/chatMode";
import { templateForLocale } from "../../lib/agent/agentCreateTemplate";
import {
  clearChatSession,
  isChatCleared,
  isWelcomeOnly,
  loadChatSession,
  persistAfterEditTruncate,
  saveChatSession,
} from "../../lib/chat/chatSessionStore";
import { mapHistoryMessages } from "../../lib/chat/mapHistoryMessages";
import { MSG_DISSOLVE_MS } from "../../components/chat/MsgDissolveOverlay";
import type {
  ArtifactDto,
  ChatAttachment,
  ChatAttachmentKind,
  ChatEmptyMode,
  ChatHistoryDto,
  ChatMessage,
  MessageTokenUsage,
  PendingInterrupt,
  ProviderDto,
} from "../../types";
import type { ContextUsageSnapshot } from "../../lib/chat/contextUsage";
import type { ChatDisplayPrefs } from "./useChatDisplayPrefs";
import type { ShowToastOptions } from "../ui/useTransientToast";
import type { MessageKey } from "../../i18n/messages";
import type { ChatRightTab } from "../../components/chat/ChatRightPanel";
import { useChatStreamBuffers } from "./useChatStreamBuffers";
import { useSend } from "./useSend";

type TFn = (key: MessageKey, vars?: Record<string, string>) => string;
type ShowToastFn = (msg: string, opts?: ShowToastOptions) => void;
type StatusPhase = "ready" | "connecting" | "generating" | "error";
type NavId = "chat" | "memory" | "workspace" | "filespace" | "skills" | "tools" | "insights" | "cron" | "providers" | "settings";

const MAX_ATTACHMENTS = 8;
const MAX_INLINE_BYTES = 4 * 1024 * 1024;

export { mapHistoryMessages } from "../../lib/chat/mapHistoryMessages";

function countChatBubbles(msgs: ChatMessage[]): number {
  return msgs.filter(
    (m) => m.id !== "welcome" && (m.role === "user" || m.role === "assistant"),
  ).length;
}

function kindFromMime(mime: string, name: string): ChatAttachmentKind {
  if (mime.startsWith("image/")) return "image";
  if (mime.startsWith("video/")) return "video";
  if (mime.startsWith("audio/")) return "audio";
  const ext = name.split(".").pop()?.toLowerCase() ?? "";
  if (["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "heic"].includes(ext)) return "image";
  if (["mp4", "webm", "mov", "mkv", "avi"].includes(ext)) return "video";
  if (["mp3", "wav", "m4a", "aac", "ogg", "flac"].includes(ext)) return "audio";
  return "file";
}

function calcTokensPerSec(completionTokens: number, durationMs: number): number | undefined {
  if (completionTokens <= 0 || durationMs <= 0) return undefined;
  const sec = Math.max(0.1, durationMs / 1000);
  return Math.round((completionTokens / sec) * 10) / 10;
}

export interface UseChatSessionDeps {
  activeProvider: ProviderDto | undefined;
  providers: ProviderDto[];
  chatMode: ChatInteractionMode;
  chatDisplayPrefsRef: RefObject<ChatDisplayPrefs>;
  locale: string;
  t: TFn;
  showTransientToast: ShowToastFn;
  nav: NavId;
  setNav: Dispatch<SetStateAction<NavId>>;
}

export function useChatSession({
  activeProvider,
  providers,
  chatMode,
  chatDisplayPrefsRef,
  locale,
  t,
  showTransientToast,
  nav,
  setNav,
}: UseChatSessionDeps) {
  // ── Core state ────────────────────────────────────────────────────────────
  const [messages, setMessages] = useState<ChatMessage[]>(() => {
    const stored = loadChatSession();
    if (stored?.messages?.length) return stored.messages;
    return [];
  });
  const [emptyMode, setEmptyMode] = useState<ChatEmptyMode>(() => {
    const stored = loadChatSession();
    return stored && !isWelcomeOnly(stored.messages) ? null : "chat";
  });
  const [input, setInput] = useState("");
  const [attachments, setAttachments] = useState<ChatAttachment[]>([]);
  const [streaming, setStreaming] = useState(false);
  const [streamPaused, setStreamPaused] = useState(false);
  const [tokenUsage, setTokenUsage] = useState<MessageTokenUsage | null>(null);
  const [contextUsage, setContextUsage] = useState<ContextUsageSnapshot | null>(null);
  const [sessionId, setSessionId] = useState<string | null>(
    () => loadChatSession()?.sessionId ?? null,
  );
  const [sessionPendingInterrupts, setSessionPendingInterrupts] = useState<PendingInterrupt[]>(
    () => loadChatSession()?.pendingInterrupts ?? [],
  );
  const [sessionReadOnly, setSessionReadOnly] = useState(false);
  const [sessionEndReason, setSessionEndReason] = useState<string | null>(null);
  const [currentTurnId, setCurrentTurnId] = useState<string | null>(null);
  const [focusMessageId, setFocusMessageId] = useState<string | null>(null);
  const [isCompacting, setIsCompacting] = useState(false);
  const [dissolvingIds, setDissolvingIds] = useState<string[]>([]);
  const [status, setStatus] = useState<"ready" | "busy" | "error">("ready");
  const [statusPhase, setStatusPhase] = useState<StatusPhase>("ready");
  const [statusDetail, setStatusDetail] = useState<string | null>(null);
  const [memoryPendingCount, setMemoryPendingCount] = useState(0);
  const [chatRightOpen, setChatRightOpen] = useState(() => {
    try {
      return localStorage.getItem("astro.chatRightOpen") === "1";
    } catch {
      return false;
    }
  });
  const [chatRightTab, setChatRightTab] = useState<ChatRightTab>("sessions");

  // ── Refs ──────────────────────────────────────────────────────────────────
  const unlistenRef = useRef<(() => void) | null>(null);
  const restoringRef = useRef(false);
  const pendingKeepChatBubblesRef = useRef<number | null>(null);
  const dissolvingIdsRef = useRef<string[]>([]);
  dissolvingIdsRef.current = dissolvingIds;
  const compactingRef = useRef(false);
  const prevStreamingRef = useRef(false);
  const lastCompactAtRef = useRef(0);
  const lastAutoCompactAttemptRef = useRef(0);
  const lastRecommendCompactToastAtRef = useRef(0);
  const memoryToastDedupeRef = useRef<{ key: string; at: number } | null>(null);
  const dissolveTimerRef = useRef<number | null>(null);

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

  // ── Send ──────────────────────────────────────────────────────────────────
  const { send } = useSend({
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
  });

  // ── Persist session ───────────────────────────────────────────────────────
  useEffect(() => {
    if (restoringRef.current || streaming) return;
    saveChatSession(sessionId, messages, sessionPendingInterrupts);
  }, [messages, sessionId, streaming, sessionPendingInterrupts]);

  useEffect(() => {
    try {
      localStorage.setItem("astro.chatRightOpen", chatRightOpen ? "1" : "0");
    } catch {
      // ignore quota / private mode
    }
  }, [chatRightOpen]);

  // ── Session events filter ─────────────────────────────────────────────────
  useEffect(() => {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) return;
    void invoke("set_session_events_filter", {
      sessionId: sessionId ?? null,
      agentId: null,
    }).catch(() => {});
  }, [sessionId]);

  // ── Memory pending count init ─────────────────────────────────────────────
  useEffect(() => {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) return;
    void (async () => {
      try {
        const rows = await invoke<{ id: string }[]>("list_pending_memory_writes");
        setMemoryPendingCount(rows?.length ?? 0);
      } catch {
        setMemoryPendingCount(0);
      }
    })();
  }, []);

  // ── Skills seeded toast ───────────────────────────────────────────────────
  useEffect(() => {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) return;
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
      .then((fn) => { unlisten = fn; })
      .catch(() => {});
    return () => { unlisten?.(); };
  }, [t, showTransientToast]);

  // ── memory-updated event ──────────────────────────────────────────────────
  useEffect(() => {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) return;
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
      .then((fn) => { unlisten = fn; })
      .catch(() => {});
    return () => { unlisten?.(); };
  }, [t, showTransientToast, chatDisplayPrefsRef]);

  // ── session_event: badge + memory refresh ────────────────────────────────
  useEffect(() => {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) return;
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
      .then((fn) => { unlisten = fn; })
      .catch(() => {});
    return () => { unlisten?.(); };
  }, [sessionId, showTransientToast, t]);

  // ── Cleanup dissolve timer ────────────────────────────────────────────────
  useEffect(() => {
    return () => {
      if (dissolveTimerRef.current != null) {
        window.clearTimeout(dissolveTimerRef.current);
      }
    };
  }, []);

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
      restored: ChatMessage[],
      pendingInterrupts: PendingInterrupt[] = [],
      endReason?: string | null,
    ) => {
      if (restored.length === 0) return false;
      restoringRef.current = true;
      currentRunIdRef.current = null;
      setCurrentTurnId(null);
      setSessionId(sid);
      setMessages(restored);
      setSessionPendingInterrupts(pendingInterrupts);
      setSessionReadOnly(!!endReason);
      setSessionEndReason(endReason ?? null);
      setEmptyMode(null);
      saveChatSession(sid, restored, pendingInterrupts);
      queueMicrotask(() => {
        restoringRef.current = false;
      });
      return true;
    },
    [currentRunIdRef],
  );

  const restoreChatHistory = useCallback(async () => {
    if (streaming || restoringRef.current) return;
    if (!isWelcomeOnly(messages)) return;
    if (pendingKeepChatBubblesRef.current != null || dissolvingIdsRef.current.length > 0) {
      return;
    }
    if (isChatCleared()) return;

    const stored = loadChatSession();
    if (stored && !isWelcomeOnly(stored.messages)) {
      applyRestoredHistory(
        stored.sessionId,
        stored.messages,
        stored.pendingInterrupts ?? [],
      );
      if (stored.sessionId) {
        void invoke<ChatHistoryDto>("get_chat_history", {
          sessionId: stored.sessionId,
          limit: 1,
        })
          .then((h) => {
            if (h.endReason) {
              setSessionReadOnly(true);
              setSessionEndReason(h.endReason);
            }
          })
          .catch(() => {});
      }
      return;
    }

    try {
      const history = await invoke<ChatHistoryDto>("get_chat_history", {
        sessionId: sessionId ?? stored?.sessionId ?? null,
        limit: 200,
      });
      if (!history.messages?.length) return;
      const restored = mapHistoryMessages(history.messages);
      if (restored.length === 0) return;
      applyRestoredHistory(history.sessionId, restored, [], history.endReason);
    } catch {
      // keep welcome page if backend unavailable
    }
  }, [applyRestoredHistory, messages, sessionId, streaming]);

  const clearLocalChatSurface = useCallback(() => {
    unlistenRef.current?.();
    unlistenRef.current = null;
    clearStreamBuffers();
    clearChatSession();
    pendingKeepChatBubblesRef.current = null;
    setSessionId(null);
    setAttachments((prev) => {
      for (const a of prev) {
        if (a.previewUrl) URL.revokeObjectURL(a.previewUrl);
      }
      return [];
    });
    setStreaming(false);
    setStreamPaused(false);
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
  }, [activeAssistantIdRef, clearStreamBuffers, currentRunIdRef, setNav]);

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
      showTransientToast(t("chat.compactBlockedStreaming"), { tone: "warning" });
      return;
    }
    if (sessionPendingInterrupts.length > 0) {
      showTransientToast(t("chat.compactBlockedInterrupt"), { tone: "warning" });
      return;
    }
    if (!sessionId) {
      showTransientToast(t("chat.compactFailed", { error: "no session" }), { tone: "error" });
      return;
    }

    compactingRef.current = true;
    setIsCompacting(true);
    let splitNewId: string | null = null;
    try {
      const res = await invoke<{ newSessionId: string; summaryPreview: string; degraded: boolean }>(
        "compact_chat_session",
        { sessionId, keepTailBubbles: 3, focus: null },
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
      saveChatSession(res.newSessionId, [], []);

      try {
        const history = await invoke<ChatHistoryDto>("get_chat_history", {
          sessionId: res.newSessionId,
          limit: 200,
        });
        const restored = mapHistoryMessages(history.messages ?? []);
        if (!applyRestoredHistory(res.newSessionId, restored, [], null)) {
          setMessages(restored);
          saveChatSession(res.newSessionId, restored, []);
        }
        lastCompactAtRef.current = Date.now();
        showTransientToast(
          res.degraded ? t("chat.compactDegraded") : t("chat.compactDone"),
          { tone: res.degraded ? "warning" : "success" },
        );
      } catch (histErr) {
        lastCompactAtRef.current = Date.now();
        showTransientToast(
          t("chat.compactHistoryFailed", {
            error: histErr instanceof Error ? histErr.message : String(histErr ?? "error"),
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

  const maybeAutoCompact = useCallback(() => {
    if (streaming || compactingRef.current || isCompacting) return;
    if (sessionPendingInterrupts.length > 0) return;
    if (!sessionId) return;

    const bubbles = messages.filter(
      (m) => m.id !== "welcome" && (m.role === "user" || m.role === "assistant"),
    ).length;
    if (bubbles < 6) return;

    const now = Date.now();
    if (now - lastAutoCompactAttemptRef.current < 60_000) return;
    if (now - lastCompactAtRef.current < 60_000) return;

    let ratio: number;
    if (tokenUsage && tokenUsage.totalTokens > 0) {
      ratio = tokenUsage.totalTokens / 128_000;
    } else {
      const chars = messages.reduce(
        (n, m) => n + (m.content?.length ?? 0) + (m.reasoning?.length ?? 0),
        0,
      );
      ratio = Math.ceil(chars / 4) / 128_000;
    }
    if (ratio < 0.5) return;

    lastAutoCompactAttemptRef.current = now;
    void runCompactSession();
  }, [
    streaming,
    isCompacting,
    sessionPendingInterrupts,
    sessionId,
    messages,
    tokenUsage,
    runCompactSession,
  ]);

  useEffect(() => {
    const wasStreaming = prevStreamingRef.current;
    prevStreamingRef.current = streaming;
    if (wasStreaming && !streaming) {
      maybeAutoCompact();
    }
  }, [streaming, maybeAutoCompact]);

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
    if (!sessionId || !streaming) return;
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

  // ── Message operations ────────────────────────────────────────────────────
  const regenerateMessage = useCallback(
    (assistantId: string) => {
      if (streaming) return;
      const idx = messages.findIndex((m) => m.id === assistantId);
      if (idx < 0 || messages[idx]?.role !== "assistant") return;
      let userIdx = -1;
      for (let i = idx - 1; i >= 0; i -= 1) {
        if (messages[i].role === "user") {
          userIdx = i;
          break;
        }
      }
      if (userIdx < 0) return;
      const userMsg = messages[userIdx];
      pendingKeepChatBubblesRef.current = countChatBubbles(messages.slice(0, userIdx + 1));
      void send({
        text: userMsg.content,
        attachments: userMsg.attachments ?? [],
        truncateTo: userIdx + 1,
        skipUserAppend: true,
        reuseUserId: userMsg.id,
      });
    },
    [messages, streaming, send],
  );

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

  const retryLastAssistant = useCallback(() => {
    if (streaming) return;
    let lastAssistantId: string | null = null;
    for (let i = messages.length - 1; i >= 0; i -= 1) {
      if (messages[i].role === "assistant") {
        lastAssistantId = messages[i].id;
        break;
      }
    }
    if (!lastAssistantId) {
      showTransientToast(t("chat.slashRetryEmpty"));
      return;
    }
    regenerateMessage(lastAssistantId);
  }, [messages, streaming, regenerateMessage, showTransientToast, t]);

  const editUserMessage = useCallback(
    (messageId: string) => {
      if (streaming || dissolvingIds.length > 0) return;
      const idx = messages.findIndex((m) => m.id === messageId);
      if (idx < 0 || messages[idx]?.role !== "user") return;
      const userMsg = messages[idx];
      const victimIds = messages.slice(idx).map((m) => m.id);
      const kept = messages.slice(0, idx);
      const bubbleStart = countChatBubbles(kept);
      const bubbleEnd = countChatBubbles(messages);
      pendingKeepChatBubblesRef.current = bubbleStart;

      const beginCut = () => {
        persistAfterEditTruncate(sessionId, kept);
        setInput(userMsg.content);
        setAttachments((userMsg.attachments ?? []).map((a) => ({ ...a })));
        setSessionPendingInterrupts([]);

        const reduced =
          typeof window !== "undefined" &&
          window.matchMedia("(prefers-reduced-motion: reduce)").matches;

        const finishCut = () => {
          setMessages((prev) => {
            const cut = prev.findIndex((m) => m.id === messageId);
            return cut < 0 ? prev : prev.slice(0, cut);
          });
          setDissolvingIds([]);
          dissolveTimerRef.current = null;
          if (bubbleStart === 0) {
            queueMicrotask(() => setEmptyMode("chat"));
          }
        };

        if (reduced) {
          finishCut();
        } else {
          setDissolvingIds(victimIds);
          if (dissolveTimerRef.current != null) {
            window.clearTimeout(dissolveTimerRef.current);
          }
          dissolveTimerRef.current = window.setTimeout(finishCut, MSG_DISSOLVE_MS);
        }

        queueMicrotask(() => {
          const el = document.querySelector<HTMLTextAreaElement>(".composer-shell textarea");
          el?.focus();
          if (el) {
            const len = el.value.length;
            el.setSelectionRange(len, len);
          }
        });
      };

      if (
        sessionId &&
        bubbleStart < bubbleEnd &&
        typeof window !== "undefined" &&
        "__TAURI_INTERNALS__" in window
      ) {
        void invoke("remove_chat_bubbles", {
          sessionId,
          start: bubbleStart,
          end: bubbleEnd,
        })
          .then(beginCut)
          .catch((e) => {
            pendingKeepChatBubblesRef.current = null;
            showTransientToast(
              t("chat.deleteFailed", {
                error: e instanceof Error ? e.message : String(e ?? "error"),
              }),
            );
          });
        return;
      }
      beginCut();
    },
    [messages, streaming, dissolvingIds.length, sessionId, showTransientToast, t],
  );

  const deleteMessage = useCallback(
    (messageId: string) => {
      if (streaming) return;
      const idx = messages.findIndex((m) => m.id === messageId);
      if (idx < 0) return;
      let end = idx + 1;
      if (messages[idx].role === "user") {
        while (end < messages.length && messages[end].role === "assistant") end += 1;
      }
      const bubbleStart = countChatBubbles(messages.slice(0, idx));
      const bubbleEnd = countChatBubbles(messages.slice(0, end));
      const next = [...messages.slice(0, idx), ...messages.slice(end)];

      const applyLocal = () => {
        setMessages(next);
        if (next.length === 0 || next.every((m) => m.id === "welcome")) {
          clearChatSession();
          queueMicrotask(() => setEmptyMode("chat"));
        } else {
          saveChatSession(sessionId, next, []);
        }
        setSessionPendingInterrupts([]);
      };

      if (
        sessionId &&
        bubbleStart < bubbleEnd &&
        typeof window !== "undefined" &&
        "__TAURI_INTERNALS__" in window
      ) {
        void invoke("remove_chat_bubbles", {
          sessionId,
          start: bubbleStart,
          end: bubbleEnd,
        })
          .then(applyLocal)
          .catch((e) => {
            showTransientToast(
              t("chat.deleteFailed", {
                error: e instanceof Error ? e.message : String(e ?? "error"),
              }),
            );
          });
        return;
      }
      applyLocal();
    },
    [messages, sessionId, streaming, showTransientToast, t],
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
      saveChatSession(newId, keep, []);
      showTransientToast(t("chat.branchDone"), { tone: "success" });
    },
    [messages, streaming, sessionId, clearStreamBuffers, showTransientToast, t, currentRunIdRef],
  );

  // ── HITL UI action ────────────────────────────────────────────────────────
  const onUiAction = useCallback(
    async (messageId: string, name: string, context: Record<string, unknown>) => {
      if (!activeProvider || sessionPendingInterrupts.length === 0) return;
      const isLocationHitl = sessionPendingInterrupts.some(
        (p) => p.reason === "location_required",
      );

      let payload: Record<string, unknown>;
      if (name === "share_location") {
        if (typeof navigator === "undefined" || !navigator.geolocation?.getCurrentPosition) {
          showTransientToast(t("chat.location.geoUnavailable"), {
            tone: "error",
          });
          return;
        }
        try {
          const pos = await new Promise<GeolocationPosition>((resolve, reject) => {
            navigator.geolocation.getCurrentPosition(resolve, reject, {
              enableHighAccuracy: true,
              timeout: 15_000,
              maximumAge: 60_000,
            });
          });
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
          showTransientToast(t("chat.location.cityRequired"), { tone: "warning" });
          return;
        }
        payload = { city };
      } else if (name === "approve") {
        payload = { approved: true };
      } else if (name === "deny") {
        payload = isLocationHitl ? { denied: true } : { approved: false };
      } else if (name === "choose") {
        const value = context.value;
        if (typeof value !== "string" || !value.trim()) return;
        payload = { value };
      } else {
        payload = { ...context };
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
            uiSurfaces: m.uiSurfaces?.map((s) => ({ ...s, status: "resolved" as const })),
          };
        }),
      );
      setSessionPendingInterrupts([]);
      if (!sessionId) {
        showTransientToast(t("chat.interrupt.pending"));
        return;
      }
      try {
        await invoke("interrupt_resume", { sessionId, resumeJson });
      } catch (e) {
        showTransientToast(e instanceof Error ? e.message : String(e ?? "HITL resume failed"));
      }
    },
    [activeProvider, sessionPendingInterrupts, sessionId, showTransientToast, t],
  );

  // ── Reset / New session ───────────────────────────────────────────────────
  const resetChatSurface = useCallback(() => {
    const sid = sessionId;
    if (sid && typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
      void invoke("chat_control", { sessionId: sid, action: "new_chat" }).catch(
        (e) => console.warn("chat_control new_chat failed", e),
      );
    }
    clearLocalChatSurface();
  }, [sessionId, clearLocalChatSurface]);

  const confirmIfStreaming = useCallback(() => {
    if (!streaming) return true;
    return window.confirm(t("chat.newSessionStreamingConfirm"));
  }, [streaming, t]);

  const startNewChat = useCallback(() => {
    if (!confirmIfStreaming()) return;
    resetChatSurface();
    setInput("");
    setEmptyMode("chat");
  }, [confirmIfStreaming, resetChatSurface]);

  const startNewAgent = useCallback(() => {
    if (!confirmIfStreaming()) return;
    resetChatSurface();
    setEmptyMode("agent");
    setInput(templateForLocale(locale));
    setChatRightOpen(false);
    if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
      void invoke("clear_pending_agent_icon", { kind: null }).catch(() => {});
    }
  }, [confirmIfStreaming, resetChatSurface, locale]);

  const skipAgentCreate = useCallback(() => {
    setEmptyMode("chat");
    setInput("");
    if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
      void invoke("clear_pending_agent_icon", { kind: null }).catch(() => {});
    }
  }, []);

  // ── Open session from file space ──────────────────────────────────────────
  const openSessionFromFilespace = useCallback(
    async (targetSessionId: string, messageId?: string | null) => {
      try {
        const hist = await invoke<ChatHistoryDto>("get_chat_history", {
          sessionId: targetSessionId,
          limit: 200,
        });
        const restored = mapHistoryMessages(hist.messages ?? []);
        const endReason = hist.endReason ?? null;
        if (restored.length > 0) {
          applyRestoredHistory(hist.sessionId ?? targetSessionId, restored, [], endReason);
        } else {
          currentRunIdRef.current = null;
          setCurrentTurnId(null);
          setSessionId(hist.sessionId ?? targetSessionId);
          setSessionReadOnly(!!endReason);
          setSessionEndReason(endReason);
        }
        const canFocus = !!messageId && restored.some((m) => m.id === messageId);
        setFocusMessageId(canFocus ? messageId! : null);
        setEmptyMode(null);
        setNav("chat");
      } catch (e) {
        setStatus("error");
        setStatusPhase("error");
        setStatusDetail(String(e));
      }
    },
    [applyRestoredHistory, currentRunIdRef, setNav],
  );

  // ── Attach artifacts ──────────────────────────────────────────────────────
  const attachArtifactsToChat = useCallback(
    async (files: ArtifactDto[], mode: "new" | "current") => {
      const usable = files.filter((f) => !f.missing);
      const converted: ChatAttachment[] = [];
      const fails: string[] = [];

      for (const f of usable) {
        try {
          const dto = await invoke<{ name: string; mime: string; size: number; base64: string }>(
            "read_file_base64",
            { path: f.path },
          );
          const kind = kindFromMime(dto.mime, dto.name);
          const shouldInline =
            (kind === "image" && dto.size <= MAX_INLINE_BYTES) ||
            (kind === "file" &&
              dto.size <= 256 * 1024 &&
              (dto.mime.startsWith("text/") ||
                /\.(txt|md|json|csv|xml|yaml|yml|toml|rs|ts|tsx|js|py|html|css)$/i.test(dto.name)));
          let previewUrl: string | undefined;
          if (kind === "image" || kind === "video") {
            try { previewUrl = convertFileSrc(f.path); } catch { previewUrl = undefined; }
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
        if (!confirmIfStreaming()) return;
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
    streaming,
    streamPaused,
    tokenUsage,
    contextUsage,
    sessionId,
    sessionPendingInterrupts,
    sessionReadOnly,
    sessionEndReason,
    currentTurnId,
    focusMessageId,
    isCompacting,
    dissolvingIds,
    status,
    statusPhase,
    statusDetail,
    memoryPendingCount,
    chatRightOpen,
    chatRightTab,
    // setters needed by App
    setInput,
    setAttachments,
    setMemoryPendingCount,
    setChatRightOpen,
    setChatRightTab,
    setFocusMessageId,
    setStatusDetail,
    // callbacks
    send,
    pauseStream,
    resumeStream,
    stopStream,
    regenerateMessage,
    undoLastExchange,
    retryLastAssistant,
    editUserMessage,
    deleteMessage,
    branchMessage,
    runCompactSession,
    onUiAction,
    resetChatSurface,
    startNewChat,
    startNewAgent,
    skipAgentCreate,
    openSessionFromFilespace,
    attachArtifactsToChat,
    applyRestoredHistory,
    confirmIfStreaming,
    prepareDeleteCurrentSession,
    clearDeletedCurrentSession: clearLocalChatSurface,
  };
}
