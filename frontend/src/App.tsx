/** 根布局：侧栏导航、聊天与各功能面板编排。 */
import { useCallback, useEffect, useRef, useState, type ComponentType, type MouseEvent as ReactMouseEvent, type SVGProps } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import AnimatedSwitch from "./components/AnimatedSwitch";
import ChatRightPanel, { type ChatRightTab } from "./components/ChatRightPanel";
import ChatView from "./components/ChatView";
import CronPanel from "./components/CronPanel";
import FileSpacePanel from "./components/FileSpacePanel";
import InsightsPanel from "./components/InsightsPanel";
import MemoryPanel from "./components/MemoryPanel";
import ModelPicker from "./components/ModelPicker";
import PreferencesPanel from "./components/PreferencesPanel";
import ProvidersPanel from "./components/ProvidersPanel";
import SkillsPanel from "./components/SkillsPanel";
import ToolsPanel from "./components/ToolsPanel";
import { Toast } from "./components/Toast";
import WorkspacePanel from "./components/WorkspacePanel";
import { MSG_DISSOLVE_MS } from "./components/MsgDissolveOverlay";
import {
  IconChat,
  IconCollapse,
  IconCron,
  IconExpand,
  IconFileSpace,
  IconInsights,
  IconMemory,
  IconNewSession,
  IconPanelClose,
  IconPanelOpen,
  IconProviders,
  IconRightPanel,
  IconSettings,
  IconSidebarIcons,
  IconSidebarLabels,
  IconSkills,
  IconTools,
  IconWorkspace,
} from "./components/NavIcons";
import { useChatDisplayPrefs } from "./hooks/useChatDisplayPrefs";
import { useChatThinkingPrefs } from "./hooks/useChatThinkingPrefs";
import { useBeautifyTips } from "./hooks/useBeautifyTips";
import { useTheme } from "./hooks/useTheme";
import { useI18n } from "./i18n/LocaleContext";
import type { MessageKey } from "./i18n/messages";
import { templateForLocale } from "./lib/agentCreateTemplate";
import {
  applyActivityUpsert,
  applyReasoningDelta,
  applySurfaceUpsert,
  sealOpenReasoning,
} from "./lib/chatTimeline";
import { elapsedSecSince } from "./lib/elapsedSec";
import { type ThinkingLevel } from "./lib/thinkingPrefs";
import {
  loadModelPrefs,
  loadPickerGlobals,
  modelPrefsToApi,
  modelPrefsToThinkingLevel,
  syncMaxModeWithThinkingLevel,
  thinkingLevelToModelPatch,
  upsertModelPrefs,
  type ModelPickerGlobals,
  type ModelRuntimePrefs,
} from "./lib/modelPrefs";
import {
  loadModelCandidates,
  selectAutoModel,
} from "./lib/autoModelSelect";
import { shouldShowThinkingControls } from "./lib/shouldShowThinkingControls";
import {
  CHAT_MODES,
  chatModeHint,
  loadChatMode,
  saveChatMode,
  type ChatInteractionMode,
} from "./lib/chatMode";
import type { SlashAction } from "./lib/composerCommands";
import { resolveComposerTurn } from "./lib/composerResolve";
import {
  normalizeContextUsageEvent,
  resolveContextWindow,
  usagePercent,
  type ContextUsageSnapshot,
} from "./lib/contextUsage";
import { zoomOrRestore } from "./lib/windowZoom";
import { syncWindowUnderlay } from "./lib/windowUnderlay";
import {
  clearChatSession,
  isChatCleared,
  isWelcomeOnly,
  loadChatSession,
  saveChatSession,
} from "./lib/chatSessionStore";
import type {
  ArtifactDto,
  ChatActivity,
  ChatActivityKind,
  ChatAttachment,
  ChatAttachmentKind,
  ChatEmptyMode,
  ChatHistoryDto,
  ChatHistoryMessageDto,
  ChatMessage,
  MessageTokenUsage,
  PendingInterrupt,
  ProviderDto,
  ProviderModelsResult,
  ProvidersStateDto,
  UiSurface,
} from "./types";

/** 单次最多附件数 */
const MAX_ATTACHMENTS = 8;
/** 图片内联 base64 上限（字节） */
const MAX_INLINE_BYTES = 4 * 1024 * 1024;

const ACTIVITY_KINDS = new Set<ChatActivityKind>([
  "tool",
  "skill",
  "mcp",
  "hook",
  "memory",
  "status",
]);

/** 计 user/assistant 聊天气泡（排除 welcome） */
function countChatBubbles(msgs: ChatMessage[]): number {
  return msgs.filter(
    (m) =>
      m.id !== "welcome" && (m.role === "user" || m.role === "assistant"),
  ).length;
}

/** 将 `get_chat_history` 富 DTO 映射为前端 ChatMessage（含 reasoning / activities） */
function mapHistoryMessages(messages: ChatHistoryMessageDto[]): ChatMessage[] {
  return messages
    .filter((m) => m.role === "user" || m.role === "assistant")
    .map((m) => {
      const activities: ChatActivity[] | undefined =
        m.activities && m.activities.length > 0
          ? m.activities.map((a) => {
              const kind = ACTIVITY_KINDS.has(a.kind as ChatActivityKind)
                ? (a.kind as ChatActivityKind)
                : "tool";
              const status =
                a.status === "running" ||
                a.status === "done" ||
                a.status === "error"
                  ? a.status
                  : undefined;
              return {
                id: a.id,
                kind,
                title: a.title,
                input: a.input ?? undefined,
                output: a.output ?? undefined,
                status,
              };
            })
          : undefined;
      const segments =
        Array.isArray(m.segments) && m.segments.length > 0
          ? m.segments
          : undefined;
      const uiSurfaces =
        Array.isArray(m.uiSurfaces) && m.uiSurfaces.length > 0
          ? m.uiSurfaces.map((s) => {
              const status =
                s.status === "resolved" || s.status === "cancelled"
                  ? s.status
                  : ("active" as const);
              return {
                messageId: s.messageId,
                activityType: s.activityType,
                operations: Array.isArray(s.operations) ? s.operations : [],
                status,
                interrupts: s.interrupts,
              };
            })
          : undefined;
      return {
        id: m.id,
        role: m.role as "user" | "assistant",
        content: m.content,
        reasoning: m.reasoning ?? undefined,
        activities,
        segments,
        uiSurfaces,
      };
    });
}

/** completion_tokens / 生成秒数，保留一位小数 */
function calcTokensPerSec(
  completionTokens: number,
  durationMs: number,
): number | undefined {
  if (completionTokens <= 0 || durationMs <= 0) return undefined;
  const sec = Math.max(0.1, durationMs / 1000);
  return Math.round((completionTokens / sec) * 10) / 10;
}

/** 由 MIME / 扩展名推断附件种类 */
function kindFromMime(mime: string, name: string): ChatAttachmentKind {
  if (mime.startsWith("image/")) return "image";
  if (mime.startsWith("video/")) return "video";
  if (mime.startsWith("audio/")) return "audio";
  const ext = name.split(".").pop()?.toLowerCase() ?? "";
  if (["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "heic"].includes(ext)) {
    return "image";
  }
  if (["mp4", "webm", "mov", "mkv", "avi"].includes(ext)) return "video";
  if (["mp3", "wav", "m4a", "aac", "ogg", "flac"].includes(ext)) return "audio";
  return "file";
}

/** 主导航项 id */
type NavId =
  | "chat"
  | "memory"
  | "workspace"
  | "filespace"
  | "skills"
  | "tools"
  | "insights"
  | "cron"
  | "providers"
  | "settings";
/** 导航图标组件类型 */
type IconComp = ComponentType<SVGProps<SVGSVGElement>>;
/** 导航项主题色 */
type Tone = "blue" | "green" | "purple" | "cyan" | "orange" | "pink" | "indigo" | "amber" | "teal";
/** 底部连接/生成状态 */
type StatusPhase = "ready" | "connecting" | "generating" | "error";

const NAV: { id: NavId; labelKey: MessageKey; Icon: IconComp; tone: Tone }[] = [
  { id: "chat", labelKey: "nav.chat", Icon: IconChat, tone: "blue" },
  { id: "memory", labelKey: "nav.memory", Icon: IconMemory, tone: "green" },
  { id: "workspace", labelKey: "nav.workspace", Icon: IconWorkspace, tone: "purple" },
  { id: "filespace", labelKey: "nav.filespace", Icon: IconFileSpace, tone: "cyan" },
  { id: "skills", labelKey: "nav.skills", Icon: IconSkills, tone: "indigo" },
  { id: "tools", labelKey: "nav.tools", Icon: IconTools, tone: "orange" },
  { id: "cron", labelKey: "nav.cron", Icon: IconCron, tone: "teal" },
  { id: "providers", labelKey: "nav.providers", Icon: IconProviders, tone: "blue" },
  { id: "insights", labelKey: "nav.insights", Icon: IconInsights, tone: "amber" },
  { id: "settings", labelKey: "nav.settings", Icon: IconSettings, tone: "pink" },
];

const PAGE_META: Record<NavId, { titleKey: MessageKey; subKey: MessageKey }> = {
  chat: { titleKey: "page.chat.title", subKey: "page.chat.sub" },
  memory: { titleKey: "page.memory.title", subKey: "page.memory.sub" },
  workspace: { titleKey: "page.workspace.title", subKey: "page.workspace.sub" },
  filespace: { titleKey: "page.filespace.title", subKey: "page.filespace.sub" },
  skills: { titleKey: "page.skills.title", subKey: "page.skills.sub" },
  tools: { titleKey: "page.tools.title", subKey: "page.tools.sub" },
  insights: { titleKey: "page.insights.title", subKey: "page.insights.sub" },
  cron: { titleKey: "page.cron.title", subKey: "page.cron.sub" },
  providers: {
    titleKey: "page.providers.title",
    subKey: "page.providers.sub",
  },
  settings: { titleKey: "page.settings.title", subKey: "page.settings.sub" },
};

export default function App() {
  const { mode, setMode, resolved, reassert } = useTheme();
  useBeautifyTips();
  const { t, locale } = useI18n();
  const { prefs: chatDisplayPrefs, setVerbosity, setToggle } = useChatDisplayPrefs();
  const chatDisplayPrefsRef = useRef(chatDisplayPrefs);
  chatDisplayPrefsRef.current = chatDisplayPrefs;
  const {
    thinkingPrefs,
    setLevel: setThinkingLevel,
  } = useChatThinkingPrefs();

  const syncComposerFromModelPrefs = useCallback(
    (prefs: ModelRuntimePrefs, globals: ModelPickerGlobals) => {
      setThinkingLevel(modelPrefsToThinkingLevel(prefs, globals));
    },
    [setThinkingLevel],
  );

  const [chatMode, setChatMode] = useState<ChatInteractionMode>(() => loadChatMode());
  const onChatModeChange = useCallback((mode: ChatInteractionMode) => {
    setChatMode(mode);
    saveChatMode(mode);
  }, []);
  const [nav, setNav] = useState<NavId>("skills");
  const [toolsInitialTab, setToolsInitialTab] = useState<"builtin" | "mcp" | null>(
    null,
  );
  const [sidebarPinned, setSidebarPinned] = useState(() => {
    try {
      return localStorage.getItem("astro.sidebarPinned") !== "0";
    } catch {
      return true;
    }
  });
  const [sidebarOpen, setSidebarOpen] = useState(() => {
    try {
      return localStorage.getItem("astro.sidebarPinned") !== "0";
    } catch {
      return true;
    }
  });
  /** 固定时是否显示文字；悬停临时展开始终显示文字。默认图标轨。 */
  const [sidebarLabels, setSidebarLabels] = useState(() => {
    try {
      return localStorage.getItem("astro.sidebarLabels") === "1";
    } catch {
      return false;
    }
  });
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
  const [tokenUsage, setTokenUsage] = useState<{
    promptTokens: number;
    completionTokens: number;
    totalTokens: number;
  } | null>(null);
  const [contextUsage, setContextUsage] = useState<ContextUsageSnapshot | null>(
    null,
  );
  const [modelContextWindow, setModelContextWindow] = useState<number | null>(
    null,
  );
  const [providers, setProviders] = useState<ProviderDto[]>([]);
  const [activeProviderId, setActiveProviderId] = useState<string | null>(null);
  const [sessionId, setSessionId] = useState<string | null>(() => {
    return loadChatSession()?.sessionId ?? null;
  });
  const [chatExpanded, setChatExpanded] = useState(false);
  const [chatRightOpen, setChatRightOpen] = useState(() => {
    try {
      return localStorage.getItem("astro.chatRightOpen") === "1";
    } catch {
      return false;
    }
  });
  const [chatRightTab, setChatRightTab] = useState<ChatRightTab>("sessions");
  const [windowMaximized, setWindowMaximized] = useState(false);
  const [status, setStatus] = useState<"ready" | "busy" | "error">("ready");
  const [statusPhase, setStatusPhase] = useState<StatusPhase>("ready");
  const [statusDetail, setStatusDetail] = useState<string | null>(null);
  const [focusMessageId, setFocusMessageId] = useState<string | null>(null);
  const [toastMsg, setToastMsg] = useState("");
  const [toastVisible, setToastVisible] = useState(false);
  /** 编辑截断时正在粒子消散的消息 */
  const [dissolvingIds, setDissolvingIds] = useState<string[]>([]);
  /** 记忆 pending 角标 */
  const [memoryPendingCount, setMemoryPendingCount] = useState(0);
  const memoryToastDedupeRef = useRef<{ key: string; at: number } | null>(null);
  /** 会话级未决 HITL interrupt（有则拒发普通消息） */
  const [sessionPendingInterrupts, setSessionPendingInterrupts] = useState<
    PendingInterrupt[]
  >(() => loadChatSession()?.pendingInterrupts ?? []);
  const unlistenRef = useRef<UnlistenFn | null>(null);
  /** 流式世代：stop / 新发送时递增，忽略迟到事件 */
  const streamGenRef = useRef(0);
  /** 当前 AG-UI run id（run_started） */
  const currentRunIdRef = useRef<string | null>(null);
  /** 当前回合 turn_id（run_started，会话切换时清空） */
  const [currentTurnId, setCurrentTurnId] = useState<string | null>(null);
  const hideTimerRef = useRef<number | null>(null);
  const dissolveTimerRef = useRef<number | null>(null);
  const dissolvingIdsRef = useRef<string[]>([]);
  dissolvingIdsRef.current = dissolvingIds;
  const zoomingRef = useRef(false);
  /** 避免恢复过程中把空欢迎页写回覆盖已存会话 */
  const restoringRef = useRef(false);
  /** 上次成功压实时间（冷却 / 状态用） */
  const lastCompactAtRef = useRef(0);
  /** 上次自动压实尝试时间（失败也计入，避免死循环） */
  const lastAutoCompactAttemptRef = useRef(0);
  const prevStreamingRef = useRef(false);
  /** 编辑/再生后下次 start_chat 应截断 DB 到的气泡数；普通发送为 null */
  const pendingKeepChatBubblesRef = useRef<number | null>(null);
  /** 流式 token / reasoning 按帧合并，避免同 tick 批量 setState 导致整段弹出 */
  const streamPendingRef = useRef<Map<string, string>>(new Map());
  const streamReasoningPendingRef = useRef<Map<string, string>>(new Map());
  /** 各助手消息开始流式的时间戳（发送时） */
  const streamStartRef = useRef<Map<string, number>>(new Map());
  /** 各助手消息首个 content token 的时间戳，用于更准的 t/s */
  const firstTokenRef = useRef<Map<string, number>>(new Map());
  /** 流式过程中暂存 usage，便于 done/stop 时一并结算速度 */
  const pendingUsageRef = useRef<Map<string, MessageTokenUsage>>(new Map());
  /** 当前流式助手消息 id，供 stop 时结算用量 */
  const activeAssistantIdRef = useRef<string | null>(null);
  const streamRafRef = useRef<number | null>(null);
  /** tool_call_delta 按帧合批：key = `${messageId}:${index}` */
  const toolDeltaPendingRef = useRef<
    Map<
      string,
      { messageId: string; index: number; id: string; name: string; args: string }
    >
  >(new Map());
  const toolDeltaRafRef = useRef<number | null>(null);
  const toolDeltaIdsRef = useRef<Map<string, string>>(new Map());

  const flushStreamTokens = useCallback(() => {
    streamRafRef.current = null;
    const batch = new Map(streamPendingRef.current);
    const reasoningBatch = new Map(streamReasoningPendingRef.current);
    streamPendingRef.current.clear();
    streamReasoningPendingRef.current.clear();
    if (batch.size === 0 && reasoningBatch.size === 0) return;
    const now = Date.now();
    setMessages((prev) =>
      prev.map((m) => {
        const extra = batch.get(m.id);
        const reasoningExtra = reasoningBatch.get(m.id);
        if (!extra && !reasoningExtra) return m;
        let next = m;
        if (reasoningExtra) {
          next = applyReasoningDelta(next, reasoningExtra, now);
        }
        // 正文首包：封口当前开放的 reasoning 段（按段 at）
        if (extra && !next.content && next.reasoning) {
          next = sealOpenReasoning(next, now);
        }
        next = {
          ...next,
          content: extra ? next.content + extra : next.content,
        };
        return next;
      }),
    );
  }, []);

  const flushToolDeltas = useCallback(() => {
    toolDeltaRafRef.current = null;
    const batch = Array.from(toolDeltaPendingRef.current.values());
    toolDeltaPendingRef.current.clear();
    if (batch.length === 0) return;
    setMessages((prev) =>
      prev.map((m) => {
        const mine = batch.filter((d) => d.messageId === m.id);
        if (mine.length === 0) return m;
        let next = m;
        for (const d of mine) {
          const mapKey = `${m.id}:${d.index}`;
          let actId = toolDeltaIdsRef.current.get(mapKey);
          const activities = next.activities ?? [];
          let idx = actId
            ? activities.findIndex((a) => a.id === actId)
            : -1;
          if (idx < 0 && d.id) {
            idx = activities.findIndex((a) => a.id === d.id);
          }
          let activity: ChatActivity;
          if (idx < 0) {
            actId = d.id || `tc-${d.index}-${Date.now()}`;
            toolDeltaIdsRef.current.set(mapKey, actId);
            activity = {
              id: actId,
              kind: "tool",
              title: d.name || `tool#${d.index}`,
              input: d.args || undefined,
              detail: d.args || undefined,
              status: "running",
              at: Date.now(),
            };
          } else {
            const cur = activities[idx]!;
            if (d.id) toolDeltaIdsRef.current.set(mapKey, d.id);
            const argsSoFar =
              cur.status === "running" ? (cur.input ?? cur.detail ?? "") : "";
            const nextArgs = d.args ? argsSoFar + d.args : cur.input ?? cur.detail;
            activity = {
              ...cur,
              id: d.id || cur.id,
              title: d.name || cur.title,
              input: nextArgs || undefined,
              detail: nextArgs || undefined,
              status: "running",
            };
          }
          next = applyActivityUpsert(next, activity);
        }
        return next;
      }),
    );
  }, []);

  const enqueueStreamToken = useCallback(
    (messageId: string, token: string) => {
      if (!token) return;
      if (!firstTokenRef.current.has(messageId)) {
        firstTokenRef.current.set(messageId, Date.now());
      }
      streamPendingRef.current.set(
        messageId,
        (streamPendingRef.current.get(messageId) ?? "") + token,
      );
      if (streamRafRef.current == null) {
        streamRafRef.current = requestAnimationFrame(flushStreamTokens);
      }
    },
    [flushStreamTokens],
  );

  const enqueueStreamReasoning = useCallback(
    (messageId: string, token: string) => {
      if (!token) return;
      streamReasoningPendingRef.current.set(
        messageId,
        (streamReasoningPendingRef.current.get(messageId) ?? "") + token,
      );
      if (streamRafRef.current == null) {
        streamRafRef.current = requestAnimationFrame(flushStreamTokens);
      }
    },
    [flushStreamTokens],
  );

  const enqueueToolDelta = useCallback(
    (
      messageId: string,
      delta: { index: number; id?: string; name?: string; arguments?: string },
    ) => {
      const key = `${messageId}:${delta.index}`;
      const prev = toolDeltaPendingRef.current.get(key);
      toolDeltaPendingRef.current.set(key, {
        messageId,
        index: delta.index,
        id: (delta.id?.trim() || prev?.id || "").trim(),
        name: (delta.name?.trim() || prev?.name || "").trim(),
        args: (prev?.args ?? "") + (delta.arguments ?? ""),
      });
      if (toolDeltaRafRef.current == null) {
        toolDeltaRafRef.current = requestAnimationFrame(flushToolDeltas);
      }
    },
    [flushToolDeltas],
  );

  const clearStreamBuffers = useCallback(() => {
    if (streamRafRef.current != null) {
      cancelAnimationFrame(streamRafRef.current);
      streamRafRef.current = null;
    }
    if (toolDeltaRafRef.current != null) {
      cancelAnimationFrame(toolDeltaRafRef.current);
      toolDeltaRafRef.current = null;
    }
    streamPendingRef.current.clear();
    streamReasoningPendingRef.current.clear();
    streamStartRef.current.clear();
    firstTokenRef.current.clear();
    pendingUsageRef.current.clear();
    activeAssistantIdRef.current = null;
    toolDeltaPendingRef.current.clear();
    toolDeltaIdsRef.current.clear();
  }, []);

  /** 将 usage + 生成速度 + 回合墙钟结算到助手消息；耗时优先首 token，否则流式起点 */
  const settleMessageUsage = useCallback((messageId: string, endedAt = Date.now()) => {
    const usage = pendingUsageRef.current.get(messageId);
    const start =
      firstTokenRef.current.get(messageId) ?? streamStartRef.current.get(messageId);
    const tokensPerSec =
      usage && start != null
        ? calcTokensPerSec(usage.completionTokens, endedAt - start)
        : undefined;
    streamStartRef.current.delete(messageId);
    firstTokenRef.current.delete(messageId);
    pendingUsageRef.current.delete(messageId);
    setMessages((prev) =>
      prev.map((m) => {
        if (m.id !== messageId) return m;
        const generationDurationSec =
          m.generationDurationSec ??
          (m.generationStartedAt != null
            ? elapsedSecSince(m.generationStartedAt, endedAt)
            : undefined);
        if (
          !usage &&
          tokensPerSec == null &&
          generationDurationSec == null
        ) {
          return m;
        }
        return {
          ...m,
          usage: usage ?? m.usage,
          tokensPerSec: tokensPerSec ?? m.tokensPerSec,
          generationDurationSec:
            generationDurationSec ?? m.generationDurationSec,
          generationStartedAt: undefined,
        };
      }),
    );
  }, []);

  const statusText = statusDetail ?? t(`status.${statusPhase}` as MessageKey);

  /** 有实质对话时持久化，便于切换导航 / 重启后恢复 */
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

  useEffect(() => {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
      return;
    }
    let unlisten: UnlistenFn | undefined;
    try {
      const win = getCurrentWindow();
      void win
        .isMaximized()
        .then(setWindowMaximized)
        .catch(() => {});
      void win
        .onResized(() => {
          void win
            .isMaximized()
            .then(setWindowMaximized)
            .catch(() => {});
        })
        .then((fn) => {
          unlisten = fn;
        })
        .catch(() => {});
    } catch {
      // 浏览器预览或 Tauri internals 未就绪
    }
    return () => {
      unlisten?.();
    };
  }, []);

  const applyRestoredHistory = useCallback(
    (
      sid: string | null,
      restored: ChatMessage[],
      pendingInterrupts: PendingInterrupt[] = [],
    ) => {
      if (restored.length === 0) return false;
      restoringRef.current = true;
      currentRunIdRef.current = null;
      setCurrentTurnId(null);
      setSessionId(sid);
      setMessages(restored);
      setSessionPendingInterrupts(pendingInterrupts);
      setEmptyMode(null);
      saveChatSession(sid, restored, pendingInterrupts);
      queueMicrotask(() => {
        restoringRef.current = false;
      });
      return true;
    },
    [],
  );

  const restoreChatHistory = useCallback(async () => {
    if (streaming || restoringRef.current) return;
    if (!isWelcomeOnly(messages)) return;
    // 用户主动「新会话」后，不要立刻从 DB 拉回旧记录
    if (isChatCleared()) return;

    const stored = loadChatSession();
    if (stored && !isWelcomeOnly(stored.messages)) {
      applyRestoredHistory(
        stored.sessionId,
        stored.messages,
        stored.pendingInterrupts ?? [],
      );
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
      applyRestoredHistory(history.sessionId, restored);
    } catch {
      // 后端/本地库不可用时保持欢迎页
    }
  }, [applyRestoredHistory, messages, sessionId, streaming]);

  /** 进入智能对话时恢复上次会话 */
  useEffect(() => {
    if (nav !== "chat") return;
    void restoreChatHistory();
  }, [nav, restoreChatHistory]);

  /** macOS 菜单栏「偏好设置」/ ⌘, → 打开偏好设置 tab */
  useEffect(() => {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
      return;
    }
    let unlisten: UnlistenFn | undefined;
    void listen("open-preferences", () => {
      setNav("settings");
    })
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => {});
    return () => {
      unlisten?.();
    };
  }, []);

  /** 记忆 SessionEvents：角标初值 + 自动 refresh 配置 */
  useEffect(() => {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
      return;
    }
    void (async () => {
      try {
        const rows = await invoke<{ id: string }[]>("list_pending_memory_writes");
        setMemoryPendingCount(rows?.length ?? 0);
      } catch {
        setMemoryPendingCount(0);
      }
    })();
  }, []);

  /** 会话变化时更新 gRPC SessionEvents 订阅过滤 */
  useEffect(() => {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
      return;
    }
    void invoke("set_session_events_filter", {
      sessionId: sessionId ?? null,
      agentId: null,
    }).catch(() => {});
  }, [sessionId]);

  useEffect(() => {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
      return;
    }
    let unlisten: UnlistenFn | undefined;
    let hideTimer: number | undefined;
    void listen<{ installed: string[]; failed: string[] }>(
      "default-skills-seeded",
      (ev) => {
        const n = ev.payload?.installed?.length ?? 0;
        if (n <= 0) return;
        setToastMsg(t("skills.defaultSeeded").replace("{n}", String(n)));
        setToastVisible(true);
        if (hideTimer) window.clearTimeout(hideTimer);
        hideTimer = window.setTimeout(() => setToastVisible(false), 4000);
      },
    )
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => {});
    return () => {
      unlisten?.();
      if (hideTimer) window.clearTimeout(hideTimer);
    };
  }, [t]);

  useEffect(() => {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
      return;
    }
    let unlisten: UnlistenFn | undefined;
    let hideTimer: number | undefined;
    void listen<{ op?: string; content?: string; new_memories?: number }>(
      "memory-updated",
      (ev) => {
        if (!chatDisplayPrefsRef.current.showMemory) return;
        const n = ev.payload?.new_memories ?? 0;
        const msg =
          ev.payload?.content?.trim() ||
          t("chat.toast.memoryUpdated").replace("{n}", String(n || 1));
        setToastMsg(msg);
        setToastVisible(true);
        if (hideTimer) window.clearTimeout(hideTimer);
        hideTimer = window.setTimeout(() => setToastVisible(false), 4000);
      },
    )
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => {});
    return () => {
      unlisten?.();
      if (hideTimer) window.clearTimeout(hideTimer);
    };
  }, [t]);

  const onTitleMouseDown = async (e: ReactMouseEvent) => {
    if (e.button !== 0) return;
    // 双击的第二次按下不要 startDragging，否则会和自定义 zoom 抢事件
    if (e.detail > 1) {
      e.preventDefault();
      return;
    }
    try {
      await getCurrentWindow().startDragging();
    } catch {
      // ignore outside Tauri
    }
  };

  const onTitleDoubleClick = async (e: ReactMouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    if (zoomingRef.current) return;
    zoomingRef.current = true;
    try {
      // 始终走自定义贴齐，绝不调用原生 toggleMaximize / zoom
      await zoomOrRestore();
    } catch {
      // ignore
    } finally {
      zoomingRef.current = false;
    }
  };

  const syncProvidersFromState = useCallback((state: ProvidersStateDto) => {
    const enabled = state.providers.filter((p) => p.enabled);
    setProviders(enabled);
    const activeId =
      state.active_provider_id &&
      enabled.some((p) => p.id === state.active_provider_id)
        ? state.active_provider_id
        : (enabled[0]?.id ?? null);
    setActiveProviderId(activeId);
  }, []);

  useEffect(() => {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
      return;
    }
    invoke<ProvidersStateDto>("get_providers_state")
      .then(syncProvidersFromState)
      .catch(() =>
        invoke<ProviderDto[]>("list_providers")
          .then((list) => {
            setProviders(list);
            if (list[0]) setActiveProviderId(list[0].id);
          })
          .catch((e) => {
            setStatus("error");
            setStatusPhase("error");
            setStatusDetail(String(e));
          }),
      );
  }, [syncProvidersFromState]);

  useEffect(() => {
    return () => {
      unlistenRef.current?.();
      if (hideTimerRef.current) window.clearTimeout(hideTimerRef.current);
      if (streamRafRef.current != null) {
        cancelAnimationFrame(streamRafRef.current);
        streamRafRef.current = null;
      }
      streamPendingRef.current.clear();
    };
  }, []);

  const openSidebar = () => {
    if (hideTimerRef.current) {
      window.clearTimeout(hideTimerRef.current);
      hideTimerRef.current = null;
    }
    setSidebarOpen(true);
  };

  const scheduleHideSidebar = () => {
    if (sidebarPinned) return;
    if (hideTimerRef.current) window.clearTimeout(hideTimerRef.current);
    hideTimerRef.current = window.setTimeout(() => {
      setSidebarOpen(false);
      hideTimerRef.current = null;
    }, 220);
  };

  const toggleSidebar = () => {
    if (hideTimerRef.current) {
      window.clearTimeout(hideTimerRef.current);
      hideTimerRef.current = null;
    }
    setSidebarPinned((pinned) => {
      const next = !pinned;
      setSidebarOpen(next);
      try {
        localStorage.setItem("astro.sidebarPinned", next ? "1" : "0");
      } catch {
        // ignore quota / private mode
      }
      return next;
    });
  };

  const toggleSidebarLabels = () => {
    setSidebarLabels((prev) => {
      const next = !prev;
      try {
        localStorage.setItem("astro.sidebarLabels", next ? "1" : "0");
      } catch {
        // ignore
      }
      return next;
    });
  };

  /** 悬停临时展开：始终出文字；固定后跟偏好（默认仅图标） */
  const sidebarVisible = sidebarOpen || sidebarPinned;
  const showSidebarLabels = sidebarVisible && (sidebarLabels || !sidebarPinned);

  const activeProvider =
    providers.find((p) => p.id === activeProviderId) ?? providers[0];

  // 从缓存模型列表解析当前模型的 context_window
  useEffect(() => {
    const providerId = activeProvider?.id;
    const modelId = activeProvider?.model;
    if (!providerId || !modelId) {
      setModelContextWindow(null);
      return;
    }
    let cancelled = false;
    void (async () => {
      try {
        const cached = await invoke<ProviderModelsResult | null>(
          "get_cached_provider_models",
          { id: providerId },
        );
        if (cancelled) return;
        const match = cached?.models?.find((m) => m.id === modelId);
        const win = match?.context_window;
        setModelContextWindow(
          typeof win === "number" && win > 0 ? win : null,
        );
      } catch {
        if (!cancelled) setModelContextWindow(null);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [activeProvider?.id, activeProvider?.model]);

  const contextWindow = resolveContextWindow(
    modelContextWindow,
    contextUsage?.contextWindow,
  );

  // caps 未知时回退 deepseek 白名单（有列表命中再传 capabilities）
  const showThinking = shouldShowThinkingControls({
    capabilities: null,
    backendId: activeProvider?.backend_id,
  });

  const onThinkingLevelChange = useCallback(
    (level: ThinkingLevel) => {
      setThinkingLevel(level);
      syncMaxModeWithThinkingLevel(level);
      if (!activeProvider) return;
      upsertModelPrefs(
        activeProvider.id,
        activeProvider.model,
        thinkingLevelToModelPatch(level),
      );
    },
    [activeProvider, setThinkingLevel],
  );

  const onToggleThinking = useCallback(() => {
    const next: ThinkingLevel =
      thinkingPrefs.level === "off" ? "high" : "off";
    onThinkingLevelChange(next);
  }, [thinkingPrefs.level, onThinkingLevelChange]);

  const send = useCallback(async (opts?: {
    /** 覆盖正文（重新生成时用前一条 user 内容） */
    text?: string;
    attachments?: ChatAttachment[];
    /** 截断到该长度后再追加 assistant（含已保留的 user） */
    truncateTo?: number;
    /** 不追加新 user 气泡（重新生成） */
    skipUserAppend?: boolean;
    /** 重新生成时沿用已有 user id，便于附件关联 */
    reuseUserId?: string;
    /** HITL resume 载荷（JSON 数组字符串） */
    resumeJson?: string;
    /** 允许空正文（仅 resume） */
    allowEmpty?: boolean;
  }) => {
    const text = (opts?.text ?? input).trim();
    const pending = opts?.attachments ?? attachments;
    const resumeJson = opts?.resumeJson?.trim() ?? "";
    if (
      sessionPendingInterrupts.length > 0 &&
      !resumeJson
    ) {
      setToastMsg(t("chat.interrupt.pending"));
      setToastVisible(true);
      if (hideTimerRef.current != null) window.clearTimeout(hideTimerRef.current);
      hideTimerRef.current = window.setTimeout(() => setToastVisible(false), 4000);
      return;
    }
    if (
      (!text && pending.length === 0 && !opts?.allowEmpty && !resumeJson) ||
      streaming ||
      !activeProvider
    ) {
      return;
    }

    // 编辑消散未完成时先截断，避免把旧气泡带进下一轮
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

    // Hermes 对齐：解析 /技能 与 @提及，注入 SKILL.md / 切 Agent / 启用 MCP
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
          mcpServers: (mcpList ?? []).map((s) => ({
            id: s.id,
            name: s.name,
          })),
        });

        if (resolved === null) {
          // 内置斜杠应由 ChatView 拦截；此处兜底不发
          return;
        }

        displayText = resolved.displayText || text;
        modelBody = resolved.modelText || text;

        if (resolved.switchAgentId) {
          try {
            await invoke("set_active_agent", {
              agentId: resolved.switchAgentId,
            });
            const hasHistory = messages.some(
              (m) => m.id !== "welcome" && (m.role === "user" || m.role === "assistant"),
            );
            setToastMsg(
              t(
                hasHistory
                  ? "chat.mentionAgentSwitchedLater"
                  : "chat.mentionAgentSwitched",
                { name: resolved.switchAgentName ?? resolved.switchAgentId },
              ),
            );
            setToastVisible(true);
            if (hideTimerRef.current != null) {
              window.clearTimeout(hideTimerRef.current);
            }
            hideTimerRef.current = window.setTimeout(
              () => setToastVisible(false),
              4000,
            );
          } catch (e) {
            console.warn("set_active_agent failed", e);
          }
        }

        if (resolved.enableMcpIds.length > 0) {
          try {
            const servers = await invoke<
              {
                id: string;
                name: string;
                enabled: boolean;
                [k: string]: unknown;
              }[]
            >("get_mcp_servers", { agentId: null });
            const want = new Set(resolved.enableMcpIds);
            const next = (servers ?? []).map((s) =>
              want.has(s.id) ? { ...s, enabled: true } : s,
            );
            await invoke("set_mcp_servers", {
              servers: next,
              agentId: null,
            });
            setToastMsg(
              t("chat.mentionMcpEnabled", {
                names: resolved.enableMcpNames.join(", "),
              }),
            );
            setToastVisible(true);
            if (hideTimerRef.current != null) {
              window.clearTimeout(hideTimerRef.current);
            }
            hideTimerRef.current = window.setTimeout(
              () => setToastVisible(false),
              4000,
            );
          } catch (e) {
            console.warn("enable mcp failed", e);
          }
        }

        if (resolved.loadedSkills.length > 0) {
          setToastMsg(
            t("chat.skillLoaded", {
              names: resolved.loadedSkills.join(", "),
            }),
          );
          setToastVisible(true);
          if (hideTimerRef.current != null) {
            window.clearTimeout(hideTimerRef.current);
          }
          hideTimerRef.current = window.setTimeout(
            () => setToastVisible(false),
            3500,
          );
        }
      } catch (e) {
        console.warn("resolveComposerTurn failed", e);
      }
    }

    const isCreatingAgent = emptyMode === "agent" && !opts?.skipUserAppend;
    const userId = opts?.reuseUserId ?? `u-${Date.now()}`;
    const assistantId = `a-${Date.now()}`;
    // 先确定 sessionId 并订阅事件，再 start_chat，避免丢早期 token
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
    setStreamPaused(false);
    setTokenUsage(null);
    setContextUsage(null);
    setStatus("busy");
    setStatusPhase("connecting");
    setStatusDetail(null);
    clearStreamBuffers();
    activeAssistantIdRef.current = assistantId;
    streamStartRef.current.set(assistantId, Date.now());
    firstTokenRef.current.delete(assistantId);
    pendingUsageRef.current.delete(assistantId);

    const contentForModel = `${
      isCreatingAgent
        ? `${modelBody}\n\n---\n${t("chat.agentCreateHint")}`
        : modelBody
    }${chatModeHint(chatMode)}`;

    try {
      unlistenRef.current?.();

      const eventName = `chat-stream-${sid}`;
      const gen = ++streamGenRef.current;
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
      }>(eventName, (event) => {
        if (streamGenRef.current !== gen) return;
        const payload = event.payload;
        if (payload.type === "token" && payload.content) {
          enqueueStreamToken(assistantId, payload.content);
        } else if (payload.type === "reasoning" && payload.content) {
          enqueueStreamReasoning(assistantId, payload.content);
          setStatusPhase("generating");
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
          setContextUsage(normalizeContextUsageEvent(payload));
        } else if (payload.type === "run_started") {
          const runId = payload.run_id ?? null;
          currentRunIdRef.current = runId;
          setCurrentTurnId(runId);
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
          if (payload.outcome_type === "hitl_waiting" || payload.outcome_type === "interrupt") {
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
                      message:
                        typeof i.message === "string" ? i.message : undefined,
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
                return sealOpenReasoning(next, Date.now());
              }),
            );
            // hitl_waiting：同回合 park，保持 streaming；旧 interrupt 结束流
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
          }
        } else if (payload.type === "tool_call_delta") {
          enqueueToolDelta(assistantId, {
            index: payload.index ?? 0,
            id: payload.id,
            name: payload.name,
            arguments: payload.arguments,
          });
          setStatusPhase("generating");
        } else if (payload.type === "tool_call") {
          // 先刷出未落地的 delta，再写完整 tool_call
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
          const activity: ChatActivity = {
            id,
            kind,
            title: name,
            input: payload.arguments_json || undefined,
            output: payload.result || undefined,
            detail: payload.result || payload.arguments_json || undefined,
            status: payload.result ? "done" : "running",
            at: Date.now(),
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
            chatDisplayPrefsRef.current.showMemory &&
            payload.operation === "background_review" &&
            payload.content
          ) {
            setToastMsg(payload.content);
            setToastVisible(true);
            window.setTimeout(() => setToastVisible(false), 4000);
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
          // 先刷出残留 token / tool delta，再结束流式状态
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
              const pending = streamPendingRef.current.get(assistantId) ?? "";
              const content = (m.content + pending).trim();
              let withUsage = sealOpenReasoning(
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
          setStreaming(false);
          setStreamPaused(false);
          setStatus("ready");
          setStatusPhase("ready");
          setStatusDetail(null);
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
          settleMessageUsage(assistantId);
          activeAssistantIdRef.current = null;
          setMessages((prev) =>
            prev.map((m) => {
              if (m.id !== assistantId) return m;
              // 保留已流式正文，仅追加错误提示
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
          setStreaming(false);
          setStreamPaused(false);
          setStatus("error");
          setStatusPhase("error");
          setStatusDetail(errMsg);
        }
      });

      for (const a of pending) {
        if (!a.dataBase64) continue;
        try {
          await invoke("save_chat_upload", {
            sessionId: sid,
            fileName: a.name,
            dataBase64: a.dataBase64,
            messageId: userId,
          });
        } catch (e) {
          console.warn("save_chat_upload failed", e);
        }
      }

      const globals = loadPickerGlobals();
      let chatProvider = activeProvider;
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
        ? modelPrefsToApi(
            loadModelPrefs(chatProvider.id, chatModel),
            globals,
          )
        : { thinkingEnabled: false, reasoningEffort: "high" as const };

      const keepChatBubbles = pendingKeepChatBubblesRef.current;
      await invoke<string>("start_chat", {
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
        attachments: pending.map((a) => ({
          name: a.name,
          mime: a.mime,
          kind: a.kind,
          size: a.size,
          dataBase64: a.dataBase64 ?? null,
        })),
      });
      // 截断成功发起后才清掉；失败则保留以便重试
      pendingKeepChatBubblesRef.current = null;
      setStatusPhase("generating");
    } catch (err) {
      clearStreamBuffers();
      setMessages((prev) =>
        prev.map((m) =>
          m.id === assistantId
            ? { ...m, content: String(err), error: true }
            : m,
        ),
      );
      setStreaming(false);
      setStreamPaused(false);
      setStatus("error");
      setStatusPhase("error");
      setStatusDetail(null);
    }
  }, [
    input,
    attachments,
    streaming,
    activeProvider,
    providers,
    sessionId,
    messages,
    emptyMode,
    sessionPendingInterrupts,
    t,
    chatMode,
    clearStreamBuffers,
    enqueueStreamToken,
    enqueueStreamReasoning,
    enqueueToolDelta,
    flushStreamTokens,
    flushToolDeltas,
    settleMessageUsage,
  ]);

  const onUiAction = useCallback(
    async (
      messageId: string,
      name: string,
      context: Record<string, unknown>,
    ) => {
      // 同回合 HITL：允许在 streaming 中提交；无 pending 则忽略
      if (!activeProvider || sessionPendingInterrupts.length === 0) {
        return;
      }
      const isLocationHitl = sessionPendingInterrupts.some(
        (p) => p.reason === "location_required",
      );

      let payload: Record<string, unknown>;
      if (name === "share_location") {
        if (
          typeof navigator === "undefined" ||
          !navigator.geolocation?.getCurrentPosition
        ) {
          setToastMsg(t("chat.location.geoUnavailable"));
          setToastVisible(true);
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
          setToastMsg(t("chat.location.geoFailed"));
          setToastVisible(true);
          return;
        }
      } else if (name === "choose_city") {
        const cityRaw = context.city;
        const city = typeof cityRaw === "string" ? cityRaw.trim() : "";
        if (!city) {
          setToastMsg(t("chat.location.cityRequired"));
          setToastVisible(true);
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
            uiSurfaces: m.uiSurfaces?.map((s) => ({
              ...s,
              status: "resolved" as const,
            })),
          };
        }),
      );
      setSessionPendingInterrupts([]);
      if (!sessionId) {
        setToastMsg(t("chat.interrupt.pending"));
        return;
      }
      try {
        await invoke("interrupt_resume", {
          sessionId,
          resumeJson,
        });
      } catch (e) {
        setToastMsg(
          e instanceof Error ? e.message : String(e ?? "HITL resume failed"),
        );
      }
    },
    [activeProvider, sessionPendingInterrupts, sessionId, t],
  );

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
      pendingKeepChatBubblesRef.current = countChatBubbles(
        messages.slice(0, userIdx + 1),
      );
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

  const showTransientToast = useCallback((msg: string) => {
    setToastMsg(msg);
    setToastVisible(true);
    if (hideTimerRef.current != null) window.clearTimeout(hideTimerRef.current);
    hideTimerRef.current = window.setTimeout(() => setToastVisible(false), 4000);
  }, []);

  /** listen session_event → toast / 角标 / 可选 refresh */
  useEffect(() => {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
      return;
    }
    let unlisten: UnlistenFn | undefined;
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
        unlisten = fn;
      })
      .catch(() => {});
    return () => {
      unlisten?.();
    };
  }, [sessionId, showTransientToast, t]);

  useEffect(() => {
    return () => {
      if (dissolveTimerRef.current != null) {
        window.clearTimeout(dissolveTimerRef.current);
      }
    };
  }, []);

  /** 编辑用户消息：正文与附件填入输入框；下方气泡粒子消散后再截断 */
  const editUserMessage = useCallback(
    (messageId: string) => {
      if (streaming || dissolvingIds.length > 0) return;
      const idx = messages.findIndex((m) => m.id === messageId);
      if (idx < 0 || messages[idx]?.role !== "user") return;
      const userMsg = messages[idx];
      const victimIds = messages.slice(idx).map((m) => m.id);
      pendingKeepChatBubblesRef.current = countChatBubbles(
        messages.slice(0, idx),
      );
      setInput(userMsg.content);
      setAttachments(
        (userMsg.attachments ?? []).map((a) => ({
          ...a,
        })),
      );
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
        const el = document.querySelector<HTMLTextAreaElement>(
          ".composer-shell textarea",
        );
        el?.focus();
        if (el) {
          const len = el.value.length;
          el.setSelectionRange(len, len);
        }
      });
    },
    [messages, streaming, dissolvingIds.length],
  );

  const deleteMessage = useCallback(
    (messageId: string) => {
      if (streaming) return;
      const idx = messages.findIndex((m) => m.id === messageId);
      if (idx < 0) return;
      let end = idx + 1;
      // 删用户消息时，一并去掉其后紧跟的助手回复（到下一条 user 之前）
      if (messages[idx].role === "user") {
        while (end < messages.length && messages[end].role === "assistant") {
          end += 1;
        }
      }
      const bubbleStart = countChatBubbles(messages.slice(0, idx));
      const bubbleEnd = countChatBubbles(messages.slice(0, end));
      const next = [...messages.slice(0, idx), ...messages.slice(end)];

      const applyLocal = () => {
        setMessages(next);
        if (next.length === 0 || next.every((m) => m.id === "welcome")) {
          queueMicrotask(() => setEmptyMode("chat"));
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

  /** 分支：复制截止该消息的历史到新会话，可继续聊 */
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
      setEmptyMode(null);
      saveChatSession(newId, keep, []);
      showTransientToast(t("chat.branchDone"));
    },
    [
      messages,
      streaming,
      sessionId,
      clearStreamBuffers,
      showTransientToast,
      t,
    ],
  );

  /** 压实：摘要旧会话并切换到含摘要+尾部的新会话 */
  const runCompactSession = useCallback(async () => {
    if (streaming) {
      showTransientToast(t("chat.compactBlockedStreaming"));
      return;
    }
    if (sessionPendingInterrupts.length > 0) {
      showTransientToast(t("chat.compactBlockedInterrupt"));
      return;
    }
    if (!sessionId) {
      showTransientToast(t("chat.compactFailed", { error: "no session" }));
      return;
    }
    try {
      const res = await invoke<{
        newSessionId: string;
        summaryPreview: string;
        degraded: boolean;
      }>("compact_chat_session", {
        sessionId,
        keepTailBubbles: 3,
        focus: null,
      });
      const history = await invoke<ChatHistoryDto>("get_chat_history", {
        sessionId: res.newSessionId,
        limit: 200,
      });
      const restored = mapHistoryMessages(history.messages ?? []);

      unlistenRef.current?.();
      unlistenRef.current = null;
      clearStreamBuffers();
      setStreaming(false);
      setStreamPaused(false);
      setFocusMessageId(null);
      currentRunIdRef.current = null;
      setCurrentTurnId(null);

      if (!applyRestoredHistory(res.newSessionId, restored)) {
        setSessionPendingInterrupts([]);
        setSessionId(res.newSessionId);
        setMessages(restored);
        setEmptyMode(null);
        saveChatSession(res.newSessionId, restored, []);
      }

      lastCompactAtRef.current = Date.now();
      showTransientToast(
        res.degraded ? t("chat.compactDegraded") : t("chat.compactDone"),
      );
    } catch (e) {
      showTransientToast(
        t("chat.compactFailed", {
          error: e instanceof Error ? e.message : String(e ?? "error"),
        }),
      );
    }
  }, [
    streaming,
    sessionPendingInterrupts,
    sessionId,
    clearStreamBuffers,
    applyRestoredHistory,
    showTransientToast,
    t,
  ]);

  /** 上下文占用超阈值时自动压实（轮次结束时检查） */
  const maybeAutoCompact = useCallback(() => {
    if (streaming) return;
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
    sessionPendingInterrupts,
    sessionId,
    messages,
    tokenUsage,
    runCompactSession,
  ]);

  // 一轮流式成功结束后尝试自动压实（streaming true→false）
  useEffect(() => {
    const wasStreaming = prevStreamingRef.current;
    prevStreamingRef.current = streaming;
    if (wasStreaming && !streaming) {
      maybeAutoCompact();
    }
  }, [streaming, maybeAutoCompact]);

  /** 撤销最近一轮 user + 紧随的 assistant */
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

  const onChatModelChange = async (providerId: string, model: string) => {
    const provider = providers.find((p) => p.id === providerId);
    if (!provider) return;

    // 先乐观更新本地，再持久化默认模型 + 活跃提供商
    setProviders((prev) =>
      prev.map((p) => (p.id === providerId ? { ...p, model } : p)),
    );
    setActiveProviderId(providerId);

    try {
      if (provider.model !== model) {
        await invoke<ProvidersStateDto>("save_provider", {
          provider: {
            id: provider.id,
            kind: provider.kind,
            display_name: provider.display_name,
            endpoint: provider.endpoint,
            model,
            enabled: provider.enabled,
          },
        });
      }
      const next = await invoke<ProvidersStateDto>("set_active_provider", {
        id: providerId,
      });
      syncProvidersFromState(next);
    } catch {
      // 本地 UI 已切换，持久化失败时忽略
    }
  };

  const toggleChatExpand = () => {
    setChatExpanded((prev) => {
      const next = !prev;
      if (next) {
        setSidebarPinned(false);
        setSidebarOpen(false);
      }
      return next;
    });
  };

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
    // 先作废当前监听，避免迟到 Done 把气泡标成空回复错误
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
  ]);

  const resetChatSurface = () => {
    const sid = sessionId;
    if (sid && typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
      void invoke("chat_control", { sessionId: sid, action: "new_chat" }).catch(
        (e) => console.warn("chat_control new_chat failed", e),
      );
    }
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
    currentRunIdRef.current = null;
    setCurrentTurnId(null);
    setNav("chat");
  };

  const confirmIfStreaming = () => {
    if (!streaming) return true;
    return window.confirm(t("chat.newSessionStreamingConfirm"));
  };

  const startNewChat = () => {
    if (!confirmIfStreaming()) return;
    resetChatSurface();
    setInput("");
    setEmptyMode("chat");
  };

  const handleSlashAction = useCallback(
    (action: SlashAction, _args?: string) => {
      const contextUsageFallback = (): number => {
        const chars = messages.reduce(
          (n, m) => n + (m.content?.length ?? 0) + (m.reasoning?.length ?? 0),
          0,
        );
        return Math.min(99, Math.round((Math.ceil(chars / 4) / 128_000) * 100));
      };

      switch (action) {
        case "new_chat":
          startNewChat();
          break;
        case "compact":
          void runCompactSession();
          break;
        case "undo":
          undoLastExchange();
          break;
        case "retry":
          retryLastAssistant();
          break;
        case "stop":
          void stopStream();
          break;
        case "status": {
          const ctx =
            tokenUsage && tokenUsage.totalTokens > 0
              ? Math.min(99, Math.round((tokenUsage.totalTokens / 128_000) * 100))
              : contextUsageFallback();
          showTransientToast(
            t("chat.slashStatusMsg", {
              session: sessionId ? sessionId.slice(0, 8) : "—",
              provider: activeProvider?.display_name ?? "—",
              model: activeProvider?.model ?? "—",
              mode: chatMode,
              thinking: thinkingPrefs.level,
              verbosity: chatDisplayPrefs.verbosity,
              ctx: String(ctx),
            }),
          );
          break;
        }
        case "usage": {
          if (!tokenUsage || tokenUsage.totalTokens <= 0) {
            showTransientToast(t("chat.slashUsageEmpty"));
          } else {
            showTransientToast(
              t("chat.slashUsageMsg", {
                total: String(tokenUsage.totalTokens),
                prompt: String(tokenUsage.promptTokens),
                completion: String(tokenUsage.completionTokens),
              }),
            );
          }
          break;
        }
        case "model":
          showTransientToast(
            t("chat.slashModelMsg", {
              provider: activeProvider?.display_name ?? "—",
              model: activeProvider?.model ?? "—",
            }),
          );
          break;
        case "verbose": {
          const order = ["compact", "normal", "detailed"] as const;
          const i = order.indexOf(chatDisplayPrefs.verbosity);
          const next = order[(i + 1) % order.length];
          setVerbosity(next);
          showTransientToast(t("chat.slashVerboseMsg", { level: next }));
          break;
        }
        case "reasoning": {
          const order = ["off", "low", "high", "max"] as const;
          const i = order.indexOf(thinkingPrefs.level);
          const next = order[(i + 1) % order.length];
          setThinkingLevel(next);
          showTransientToast(t("chat.slashReasoningMsg", { level: next }));
          break;
        }
        case "mode": {
          const i = CHAT_MODES.indexOf(chatMode);
          const next = CHAT_MODES[(i + 1) % CHAT_MODES.length];
          onChatModeChange(next);
          showTransientToast(t("chat.slashModeMsg", { mode: next }));
          break;
        }
        case "nav_tools":
          setToolsInitialTab("builtin");
          setNav("tools");
          break;
        case "nav_skills":
          setNav("skills");
          break;
        case "nav_mcp":
          setToolsInitialTab("mcp");
          setNav("tools");
          break;
        case "nav_memory":
          setNav("memory");
          break;
        case "memory_list": {
          void (async () => {
            try {
              const rows = await invoke<
                {
                  id: string;
                  action: string;
                  target: string;
                  source: string;
                }[]
              >("list_pending_memory_writes");
              if (!rows?.length) {
                showTransientToast(t("memory.pending.emptyTitle"));
                setMemoryPendingCount(0);
                return;
              }
              setMemoryPendingCount(rows.length);
              const lines = rows
                .slice(0, 5)
                .map((r) => `${r.id.slice(0, 8)} ${r.action}/${r.target} (${r.source})`);
              const more =
                rows.length > 5 ? ` …+${rows.length - 5}` : "";
              showTransientToast(`${lines.join(" · ")}${more}`);
            } catch (e) {
              showTransientToast(String(e));
            }
          })();
          break;
        }
        case "memory_approve": {
          void (async () => {
            try {
              const id = (_args ?? "").trim();
              const msg =
                !id || id === "all"
                  ? await invoke<string>("approve_all_pending_memory_writes")
                  : await invoke<string>("approve_pending_memory_write", { id });
              showTransientToast(msg || t("memory.pending.approved"));
              if (sessionId) {
                try {
                  const settings = await invoke<{ autoRefreshOnUpdate: boolean }>(
                    "get_memory_settings",
                  );
                  if (settings.autoRefreshOnUpdate !== false) {
                    await invoke("refresh_memory", {
                      agentId: null,
                      sessionId,
                    });
                  }
                } catch {
                  // ignore refresh errors
                }
              }
            } catch (e) {
              showTransientToast(String(e));
            }
          })();
          break;
        }
        case "memory_reject": {
          void (async () => {
            try {
              const id = (_args ?? "").trim();
              if (!id || id === "all") {
                const msg = await invoke<string>("reject_all_pending_memory_writes");
                showTransientToast(msg);
              } else {
                await invoke("reject_pending_memory_write", { id });
                showTransientToast(t("memory.pending.rejected"));
              }
            } catch (e) {
              showTransientToast(String(e));
            }
          })();
          break;
        }
        case "memory_refresh": {
          void (async () => {
            try {
              await invoke("refresh_memory", {
                agentId: null,
                sessionId: sessionId ?? null,
              });
              showTransientToast(t("memory.refresh.done"));
            } catch (e) {
              showTransientToast(String(e));
            }
          })();
          break;
        }
        case "memory_help":
          showTransientToast(t("memory.slash.help"));
          break;
        case "nav_insights":
          setNav("insights");
          break;
        case "nav_providers":
          setNav("providers");
          break;
        case "nav_settings":
          setNav("settings");
          break;
        case "open_context":
          setChatRightTab("context");
          setChatRightOpen(true);
          setNav("chat");
          break;
        default:
          break;
      }
    },
    [
      messages,
      undoLastExchange,
      retryLastAssistant,
      stopStream,
      runCompactSession,
      tokenUsage,
      sessionId,
      activeProvider,
      chatMode,
      thinkingPrefs.level,
      chatDisplayPrefs.verbosity,
      showTransientToast,
      t,
      setVerbosity,
      setThinkingLevel,
      onChatModeChange,
    ],
  );

  const attachArtifactsToChat = async (
    files: ArtifactDto[],
    mode: "new" | "current",
  ) => {
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
  };

  const startNewAgent = () => {
    if (!confirmIfStreaming()) return;
    resetChatSurface();
    setEmptyMode("agent");
    setInput(templateForLocale(locale));
    // 创建引导已在主区展示，避免再强制拉开右侧栏抢视线
    setChatRightOpen(false);
    if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
      void invoke("clear_pending_agent_icon", { kind: null }).catch(() => {});
    }
  };

  const skipAgentCreate = () => {
    // 创建流程进入时已清空消息；取消后回到欢迎页，避免空 message-list 白屏
    setEmptyMode("chat");
    setInput("");
    if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
      void invoke("clear_pending_agent_icon", { kind: null }).catch(() => {});
    }
  };

  const openSessionFromFilespace = async (
    targetSessionId: string,
    messageId?: string | null,
  ) => {
    try {
      const hist = await invoke<ChatHistoryDto>("get_chat_history", {
        sessionId: targetSessionId,
        limit: 200,
      });
      const restored = mapHistoryMessages(hist.messages ?? []);
      if (restored.length > 0) {
        applyRestoredHistory(hist.sessionId ?? targetSessionId, restored);
      } else {
        currentRunIdRef.current = null;
        setCurrentTurnId(null);
        setSessionId(hist.sessionId ?? targetSessionId);
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
  };

  const goNav = (id: NavId) => {
    setNav(id);
  };

  const meta = PAGE_META[nav];
  const activeTone = NAV.find((n) => n.id === nav)?.tone ?? "blue";

  useEffect(() => {
    document.documentElement.setAttribute("data-tone", activeTone);
    // 切 tab 只改 tone，再断言一次 theme，避免亮色被冲成暗色
    reassert();
  }, [activeTone, reassert]);

  useEffect(() => {
    void syncWindowUnderlay(resolved, activeTone);
  }, [resolved, activeTone]);

  const ActiveIcon = NAV.find((n) => n.id === nav)?.Icon ?? IconChat;

  return (
    <div
      className={`app-shell ${chatExpanded && nav === "chat" ? "is-chat-expanded" : ""} ${windowMaximized ? "is-maximized" : ""}`}
      data-tone={activeTone}
    >
      <div
        className="native-drag-region"
        onMouseDown={(e) => void onTitleMouseDown(e)}
        onDoubleClick={(e) => void onTitleDoubleClick(e)}
        aria-hidden
      />

      {!sidebarPinned && (
        <div
          className="sidebar-hotzone"
          onMouseEnter={openSidebar}
          aria-hidden
        />
      )}

      <div className="body-row">
        <aside
          className={`sidebar ${sidebarOpen || sidebarPinned ? "is-open" : "is-collapsed"} ${sidebarPinned ? "is-pinned" : ""} ${showSidebarLabels ? "is-labels" : "is-icons"}`}
          onMouseEnter={openSidebar}
          onMouseLeave={scheduleHideSidebar}
        >
          <div className="sidebar-brand">
            <div className="sidebar-logo" data-tone={activeTone}>
              iC
            </div>
            <div className="sidebar-brand-text">Astro Agent</div>
            <div className="sidebar-brand-actions">
              <button
                type="button"
                className="sidebar-pin-btn"
                data-tone={activeTone}
                onClick={toggleSidebarLabels}
                title={sidebarLabels ? t("sidebar.hideLabels") : t("sidebar.showLabels")}
                aria-label={
                  sidebarLabels ? t("sidebar.hideLabelsAria") : t("sidebar.showLabelsAria")
                }
                aria-pressed={sidebarLabels}
              >
                {sidebarLabels ? (
                  <IconSidebarIcons width={15} height={15} />
                ) : (
                  <IconSidebarLabels width={15} height={15} />
                )}
              </button>
              <button
                type="button"
                className="sidebar-pin-btn"
                data-tone={activeTone}
                onClick={toggleSidebar}
                title={sidebarPinned ? t("sidebar.unpin") : t("sidebar.pin")}
                aria-label={
                  sidebarPinned ? t("sidebar.unpinAria") : t("sidebar.pinAria")
                }
                aria-pressed={sidebarPinned}
              >
                {sidebarPinned ? (
                  <IconPanelClose width={15} height={15} />
                ) : (
                  <IconPanelOpen width={15} height={15} />
                )}
              </button>
            </div>
          </div>
          {NAV.map((item) => {
            const label = t(item.labelKey);
            const pendingBadge =
              item.id === "memory" && memoryPendingCount > 0
                ? memoryPendingCount > 99
                  ? "99+"
                  : String(memoryPendingCount)
                : null;
            return (
              <button
                key={item.id}
                className={`nav-item ${nav === item.id ? "active" : ""}`}
                data-tone={item.tone}
                onClick={() => goNav(item.id)}
                {...(showSidebarLabels
                  ? {}
                  : { "data-tip": label, "data-tip-pos": "right" as const })}
                aria-label={
                  pendingBadge
                    ? `${label} (${pendingBadge})`
                    : label
                }
              >
                <span className="nav-icon" aria-hidden>
                  <item.Icon />
                  {pendingBadge ? (
                    <span className="nav-badge">{pendingBadge}</span>
                  ) : null}
                </span>
                <span className="nav-label">{label}</span>
              </button>
            );
          })}
        </aside>

        <section className="content-pane">
          <div className="content-header">
            <div className="content-heading">
              <div className="page-title-block">
                <div className="page-title-icon" data-tone={activeTone} aria-hidden>
                  <ActiveIcon width={15} height={15} />
                </div>
                <div>
                  <h1 className="content-title" data-tone={activeTone}>
                    {t(meta.titleKey)}
                  </h1>
                  <p className="content-sub">{t(meta.subKey)}</p>
                </div>
              </div>
            </div>
            <div className="header-actions">
              <span className="status-chip">
                <span className={`status-dot ${status}`} />
                {activeProvider?.display_name ?? t("status.none")} · {statusText}
              </span>
              {nav === "chat" && (
                <ModelPicker
                  providers={providers}
                  value={activeProviderId}
                  onChange={(id, model) => void onChatModelChange(id, model)}
                  onActivePrefsChange={syncComposerFromModelPrefs}
                  disabled={streaming}
                />
              )}
              {nav === "chat" && (
                <div className="chat-header-tools">
                  <button
                    type="button"
                    className={`header-icon-btn ${chatExpanded ? "is-active" : ""}`}
                    onClick={toggleChatExpand}
                    title={chatExpanded ? t("chat.collapse") : t("chat.expand")}
                    aria-label={chatExpanded ? t("chat.collapse") : t("chat.expand")}
                    aria-pressed={chatExpanded}
                  >
                    {chatExpanded ? (
                      <IconCollapse width={16} height={16} />
                    ) : (
                      <IconExpand width={16} height={16} />
                    )}
                  </button>
                  <button
                    type="button"
                    className="header-icon-btn"
                    onClick={startNewAgent}
                    title={t("chat.newAgent")}
                    aria-label={t("chat.newAgent")}
                  >
                    <IconNewSession width={16} height={16} />
                  </button>
                  <button
                    type="button"
                    className={`header-icon-btn ${chatRightOpen ? "is-active" : ""}`}
                    onClick={() => setChatRightOpen((open) => !open)}
                    title={t("chat.rightPanel.toggle")}
                    aria-label={t("chat.rightPanel.toggle")}
                    aria-pressed={chatRightOpen}
                  >
                    <IconRightPanel width={16} height={16} />
                  </button>
                </div>
              )}
            </div>
          </div>
          <div className="page-body">
            <AnimatedSwitch switchKey={nav} className="anim-switch--fill">
              {nav === "chat" && (
                <div className="chat-layout-with-right">
                  <div className="chat-main">
                    <ChatView
                      messages={messages}
                      input={input}
                      attachments={attachments}
                      streaming={streaming}
                      streamPaused={streamPaused}
                      displayPrefs={chatDisplayPrefs}
                      emptyMode={emptyMode}
                      focusMessageId={focusMessageId}
                      onFocusConsumed={() => setFocusMessageId(null)}
                      onInputChange={setInput}
                      onAttachmentsChange={setAttachments}
                      onSend={send}
                      pendingInterrupts={sessionPendingInterrupts}
                      onUiAction={onUiAction}
                      onPauseStream={pauseStream}
                      onResumeStream={resumeStream}
                      onStopStream={stopStream}
                      onNewChat={startNewChat}
                      onSkipAgentCreate={skipAgentCreate}
                      onPickWelcomePrompt={(prompt) => setInput(prompt)}
                      showThinkingControls={showThinking}
                      thinkingPrefs={thinkingPrefs}
                      onToggleThinking={onToggleThinking}
                      onThinkingLevelChange={onThinkingLevelChange}
                      onOpenMcpSettings={() => {
                        setToolsInitialTab("mcp");
                        setNav("tools");
                      }}
                      chatMode={chatMode}
                      onChatModeChange={onChatModeChange}
                      onOpenContext={() => {
                        setChatRightTab("context");
                        setChatRightOpen(true);
                      }}
                      onRegenerateMessage={regenerateMessage}
                      onEditUserMessage={editUserMessage}
                      dissolvingIds={dissolvingIds}
                      onDeleteMessage={deleteMessage}
                      onBranchMessage={(id) => void branchMessage(id)}
                      onSlashAction={handleSlashAction}
                      contextUsage={contextUsage}
                      contextWindow={contextWindow}
                      contextUsagePercent={
                        contextUsage
                          ? usagePercent(contextUsage.totalTokens, contextWindow)
                          : null
                      }
                    />
                  </div>
                  {chatRightOpen && (
                    <ChatRightPanel
                      tab={chatRightTab}
                      onTabChange={setChatRightTab}
                      onClose={() => setChatRightOpen(false)}
                      sessionId={sessionId}
                      turnId={currentTurnId}
                      messages={messages}
                      tokenUsage={tokenUsage}
                      contextUsage={contextUsage}
                      contextWindow={contextWindow}
                      onOpenSession={(id) => void openSessionFromFilespace(id)}
                      onOpenMemory={() => setNav("memory")}
                      onOpenSkills={() => setNav("skills")}
                    />
                  )}
                </div>
              )}
              {nav === "memory" && (
                <MemoryPanel
                  onClose={() => setNav("chat")}
                  sessionId={sessionId}
                />
              )}
              {nav === "workspace" && (
                <WorkspacePanel onClose={() => setNav("chat")} />
              )}
              {nav === "filespace" && (
                <FileSpacePanel
                  active={nav === "filespace"}
                  onOpenSession={openSessionFromFilespace}
                  onAttachFiles={attachArtifactsToChat}
                  onClose={() => setNav("chat")}
                />
              )}
              {nav === "skills" && (
                <SkillsPanel
                  active={nav === "skills"}
                  onInstallWithAgent={(prompt) => {
                    setInput(prompt);
                    setNav("chat");
                  }}
                />
              )}
              {nav === "settings" && (
                <PreferencesPanel
                  mode={mode}
                  onChange={setMode}
                  tone={activeTone}
                  chatDisplayPrefs={chatDisplayPrefs}
                  onChatVerbosityChange={setVerbosity}
                  onChatToggleChange={setToggle}
                  activeSessionId={sessionId ?? undefined}
                />
              )}
              {nav === "tools" && (
                <ToolsPanel
                  active={nav === "tools"}
                  initialTab={toolsInitialTab}
                  onInitialTabConsumed={() => setToolsInitialTab(null)}
                />
              )}
              {nav === "insights" && (
                <InsightsPanel active={nav === "insights"} />
              )}
              {nav === "cron" && (
                <CronPanel
                  active={nav === "cron"}
                  providers={providers.map((p) => ({
                    id: p.id,
                    name: p.display_name,
                    model: p.model,
                  }))}
                  activeProviderId={activeProviderId}
                />
              )}
              {nav === "providers" && (
                <ProvidersPanel
                  active={nav === "providers"}
                  onStateChange={syncProvidersFromState}
                />
              )}
            </AnimatedSwitch>
          </div>
        </section>
      </div>
      <Toast message={toastMsg} visible={toastVisible} />
    </div>
  );
}
