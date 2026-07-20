/** 聊天主视图：消息列表、输入框与流式状态。 */
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ChangeEvent,
  type DragEvent,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { useClampPopover } from "../../hooks/ui/useClampPopover";
import {
  ArrowDown,
  ArrowUp,
  AtSign,
  ChartPie,
  Check,
  ChevronDown,
  Copy,
  File,
  FileVideo,
  GitBranch,
  Image,
  Infinity as InfinityIcon,
  Layers2,
  Lightbulb,
  ListTree,
  MessageCircle,
  Music2,
  Paperclip,
  Pause,
  Pencil,
  Play,
  RefreshCw,
  SendHorizontal,
  Slash,
  Square,
  Trash2,
} from "lucide-react";
import {
  isActivityVisible,
  type ChatDisplayPrefs,
} from "../../hooks/chat/useChatDisplayPrefs";
import { useI18n } from "../../i18n/LocaleContext";
import {
  findSlotAt,
  firstSlotValue,
  listTemplateSegments,
  nextEmptySlot,
  prepareAgentCreateSend,
  prevEmptySlot,
} from "../../lib/agent/agentCreateTemplate";
import {
  CHAT_MODES,
  type ChatInteractionMode,
} from "../../lib/chat/chatMode";
import type { QueuedFollowUp } from "../../lib/chat/followUpQueue";
import type { ContextUsageSnapshot } from "../../lib/chat/contextUsage";
import { ChatMediaAttachProvider } from "../../contexts/ChatMediaAttachContext";
import {
  attachmentsFromOsClipboard,
  filesFromClipboardRead,
  pathToAttachment,
  pathsFromClipboardText,
  pathsFromDataTransfer,
  pathsToAttachments,
} from "../../lib/chat/chatPaste";
import type { ChatThinkingPrefs, ThinkingLevel } from "../../lib/chat/thinkingPrefs";
import type {
  ChatActivity,
  ChatAttachment,
  ChatAttachmentKind,
  ChatEmptyMode,
  ChatMessage,
  InstalledSkill,
  MessageTokenUsage,
  PendingInterrupt,
} from "../../types";
import { AgentCreateGuide } from "../agents/AgentCreateGuide";
import AgentAvatar from "../agents/AgentAvatar";
import ChatMessageNav from "./ChatMessageNav";
import { ChatMarkdown } from "./ChatMarkdown";
import { ChatWelcome } from "./ChatWelcome";
import {
  ComposerPalette,
  type PaletteItem,
  type PaletteKind,
} from "./ComposerPalette";
import ComposerMcpMenu from "./ComposerMcpMenu";
import ContextUsagePopover from "./ContextUsagePopover";
import McpIcon from "../icons/McpIcon";
import { ModelBrandIcon } from "../icons/ProviderIcons";
import MsgActivity from "./MsgActivity";
import MsgDissolveOverlay from "./MsgDissolveOverlay";
import MsgReasoning from "./MsgReasoning";
import MsgStreamLoader from "./MsgStreamLoader";
import { MsgTimeline, MsgTimelineStep, type MsgTimelineKind } from "./MsgTimeline";
import { useMcpTools } from "../../hooks/providers/useMcpTools";
import { useTypingPlaceholder } from "../../hooks/chat/useTypingPlaceholder";
import A2UIRenderer from "../../a2ui/A2UIRenderer";
import LocationA2UISurface from "./LocationA2UISurface";
import A2UISurfaceCard from "./A2UISurfaceCard";
import { formatElapsedSec } from "../../lib/chat/elapsedSec";
import { coalesceReasoningSegments } from "../../lib/chat/chatTimeline";
import { isLocationRequiredSurface } from "../../lib/chat/locationSurface";
import {
  isAgentIconSrc,
  type AgentIconInfo,
} from "../../lib/agent/agentIcons";
import {
  buildMentionCandidates,
  buildSlashPaletteEntries,
  parseSlashInput,
  type SlashAction,
} from "../../lib/chat/composerCommands";

/** 格式化 token/s 展示（整数不带小数） */
function formatTokenSpeed(n: number): string {
  return Number.isInteger(n) ? String(n) : n.toFixed(1);
}

/** 单条消息底部的 token 用量、回合耗时与生成速度 */
function MessageTokenStats({
  usage,
  tokensPerSec,
  generationDurationSec,
}: {
  usage?: MessageTokenUsage;
  tokensPerSec?: number;
  generationDurationSec?: number;
}) {
  const { t } = useI18n();
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
  const label = hasUsage
    ? t("chat.tokenStats", {
        total: String(usage!.totalTokens),
        prompt: String(usage!.promptTokens),
        completion: String(usage!.completionTokens),
      })
    : null;
  const durationLabel = hasDuration
    ? t("chat.generationDuration", {
        s: formatElapsedSec(generationDurationSec!),
      })
    : null;
  const aria = t("chat.tokenStatsAria", {
    total: String(usage?.totalTokens ?? 0),
    prompt: String(usage?.promptTokens ?? 0),
    completion: String(usage?.completionTokens ?? 0),
    speed: speed ?? "—",
  });

  return (
    <div className="msg-token-stats" aria-label={aria}>
      {durationLabel ? (
        <span className="msg-token-stats-duration">{durationLabel}</span>
      ) : null}
      {label ? <span className="msg-token-stats-usage">{label}</span> : null}
      {speed != null ? (
        <span className="msg-token-stats-speed">
          {t("chat.tokenSpeed", { n: speed })}
        </span>
      ) : null}
    </div>
  );
}

/** ChatView 入参：消息列表、输入态与流式控制回调 */
type Props = {
  /** 当前会话消息（含欢迎占位） */
  messages: ChatMessage[];
  /** 输入框文本 */
  input: string;
  /** 待发送附件 */
  attachments: ChatAttachment[];
  /** 是否正在流式生成 */
  streaming: boolean;
  /** 流是否已暂停 */
  streamPaused?: boolean;
  /** 禁止发送（压实中 / 只读会话等） */
  sendBlocked?: boolean;
  /** 禁止发送时的输入框占位/原因文案 */
  sendBlockedReason?: string;
  /** 聊天展示偏好（详细度等） */
  displayPrefs: ChatDisplayPrefs;
  /** 空状态模式：欢迎 / 创建 Agent / 正常 */
  emptyMode: ChatEmptyMode;
  /** 需要滚入视口的消息 id（用后应调用 onFocusConsumed） */
  focusMessageId?: string | null;
  /** 焦点滚动完成后由父组件清空 focusMessageId */
  onFocusConsumed?: () => void;
  onInputChange: (v: string) => void;
  onAttachmentsChange: (next: ChatAttachment[]) => void;
  /** 发送当前输入 */
  onSend: (opts?: { text?: string }) => void;
  /** Agent/Plan/Ask 流式中的 follow-up 队列 */
  queuedFollowUps?: QueuedFollowUp[];
  onRemoveQueuedFollowUp?: (id: string) => void;
  onUpdateQueuedFollowUpText?: (id: string, text: string) => void;
  onMoveQueuedFollowUp?: (id: string, dir: -1 | 1) => void;
  /** 会话级未决 interrupt（有则禁用普通发送） */
  pendingInterrupts?: PendingInterrupt[];
  /** A2UI 卡片动作（approve / deny / choose） */
  onUiAction?: (
    messageId: string,
    name: string,
    context: Record<string, unknown>,
  ) => void;
  onPauseStream?: () => void;
  onResumeStream?: () => void;
  onStopStream?: () => void;
  /** 新建空白会话 */
  onNewChat: () => void;
  /** 跳过创建 Agent 引导 */
  onSkipAgentCreate: () => void;
  /** 点击欢迎页示例 Prompt */
  onPickWelcomePrompt: (prompt: string) => void;
  /** DeepSeek 等支持 thinking 时显示输入框控件 */
  showThinkingControls?: boolean;
  thinkingPrefs: ChatThinkingPrefs;
  onToggleThinking: () => void;
  onThinkingLevelChange: (level: ThinkingLevel) => void;
  /** MCP 菜单作用域 Agent；缺省走 workspace */
  agentId?: string | null;
  /** 当前聊天模型 id：助手无自定义头像时用作品牌图标 */
  modelId?: string | null;
  /** 打开 Tools 面板 MCP tab */
  onOpenMcpSettings?: () => void;
  /** Agent / Plan / Ask / MultiTask */
  chatMode: ChatInteractionMode;
  onChatModeChange: (mode: ChatInteractionMode) => void;
  /** 打开右侧上下文面板 */
  onOpenContext: () => void;
  /** 简易上下文占用 0–100，用于按钮提示 */
  contextUsagePercent?: number | null;
  /** 本轮上下文分层占用快照；无则浮层空态 */
  contextUsage?: ContextUsageSnapshot | null;
  /** 模型上下文窗口（tokens），默认 128K */
  contextWindow?: number;
  /** 重新生成该条 assistant 回复（基于前一条 user） */
  onRegenerateMessage?: (messageId: string) => void;
  /** 编辑用户消息并重发（内容填回输入框，截断该条及之后） */
  onEditUserMessage?: (messageId: string) => void;
  /** 正在粒子消散的消息 id（编辑截断中） */
  dissolvingIds?: string[];
  /** 删除该条消息 */
  onDeleteMessage?: (messageId: string) => void;
  /** 从该条消息分支新会话（复制历史到新 session） */
  onBranchMessage?: (messageId: string) => void;
  /** Hermes 风格斜杠命令执行（不含 insert_skill / help 本地处理） */
  onSlashAction?: (action: SlashAction, args?: string) => void;
};

/** 输入框 `/` 或 `@` 触发的补全状态 */
type TriggerState = {
  kind: "slash" | "mention";
  /** 触发符在 input 中的起始下标 */
  start: number;
  /** 触发符后的查询串 */
  query: string;
};

/** 根据光标前文本检测 `/` 斜杠命令或 `@` 提及触发 */
function detectTrigger(text: string, caret: number): TriggerState | null {
  const before = text.slice(0, caret);
  const slash = /(^|[\s])\/([^\s]*)$/.exec(before);
  if (slash) {
    return {
      kind: "slash",
      start: caret - slash[2].length - 1,
      query: slash[2],
    };
  }
  const mention = /(^|[\s])@([^\s]*)$/.exec(before);
  if (mention) {
    return {
      kind: "mention",
      start: caret - mention[2].length - 1,
      query: mention[2],
    };
  }
  return null;
}

/** 单次最多附件数 */
const MAX_ATTACHMENTS = 8;
/** 图片内联 base64 上限（字节） */
const MAX_INLINE_BYTES = 4 * 1024 * 1024;

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

/** 人类可读的文件大小 */
function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/** 读取文件为纯 base64（去掉 data URL 前缀） */
function fileToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const result = String(reader.result ?? "");
      const comma = result.indexOf(",");
      resolve(comma >= 0 ? result.slice(comma + 1) : result);
    };
    reader.onerror = () => reject(reader.error ?? new Error("read failed"));
    reader.readAsDataURL(file);
  });
}

/** 将本地 File 转为聊天附件（小图/文本可内联 base64） */
async function fileToAttachment(file: File): Promise<ChatAttachment> {
  const kind = kindFromMime(file.type || "", file.name);
  const id = `att-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
  const previewUrl =
    kind === "image" || kind === "video" ? URL.createObjectURL(file) : undefined;

  let dataBase64: string | undefined;
  const shouldInline =
    (kind === "image" && file.size <= MAX_INLINE_BYTES) ||
    (kind === "file" &&
      file.size <= 256 * 1024 &&
      (file.type.startsWith("text/") ||
        /\.(txt|md|json|csv|xml|yaml|yml|toml|rs|ts|tsx|js|py|html|css)$/i.test(
          file.name,
        )));

  if (shouldInline) {
    try {
      dataBase64 = await fileToBase64(file);
    } catch {
      dataBase64 = undefined;
    }
  }

  return {
    id,
    name: file.name,
    mime: file.type || "application/octet-stream",
    kind,
    size: file.size,
    previewUrl,
    dataBase64,
  };
}

/** 附件种类对应小图标 */
function AttachmentGlyph({ kind }: { kind: ChatAttachmentKind }) {
  if (kind === "image") {
    return <Image size={16} strokeWidth={2} aria-hidden />;
  }
  if (kind === "video") {
    return <FileVideo size={16} strokeWidth={2} aria-hidden />;
  }
  if (kind === "audio") {
    return <Music2 size={16} strokeWidth={2} aria-hidden />;
  }
  return <File size={16} strokeWidth={2} aria-hidden />;
}

/** 消息内附件缩略图条 */
function MessageAttachments({ items }: { items: ChatAttachment[] }) {
  if (!items.length) return null;
  return (
    <div className="msg-attachments">
      {items.map((att) => (
        <div key={att.id} className="msg-attachment" data-kind={att.kind}>
          {att.kind === "image" && att.previewUrl ? (
            <img src={att.previewUrl} alt={att.name} className="msg-attachment-thumb" />
          ) : att.kind === "video" && att.previewUrl ? (
            <video src={att.previewUrl} className="msg-attachment-thumb" muted />
          ) : (
            <span className="msg-attachment-icon" data-kind={att.kind}>
              <AttachmentGlyph kind={att.kind} />
            </span>
          )}
          <div className="msg-attachment-meta">
            <span className="msg-attachment-name">{att.name}</span>
            <span className="msg-attachment-size">{formatSize(att.size)}</span>
          </div>
        </div>
      ))}
    </div>
  );
}

/** 消息悬停操作（复制/再生/删除/分支） */
/** 消息悬停操作（复制 / 再生或编辑 / 删除 / 分支） */
function MessageActions({
  messageId,
  content,
  role,
  disabled,
  onRegenerate,
  onEdit,
  onDelete,
  onBranch,
}: {
  messageId: string;
  content: string;
  role: "user" | "assistant";
  disabled?: boolean;
  onRegenerate?: (messageId: string) => void;
  onEdit?: (messageId: string) => void;
  onDelete?: (messageId: string) => void;
  onBranch?: (messageId: string) => void;
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
    <div className="msg-actions" role="toolbar" aria-label={t("chat.messageActions")}>
      <button
        type="button"
        className={`msg-action-btn ${copied ? "is-copied" : ""}`}
        disabled={disabled || !content}
        onClick={() => void onCopy()}
        aria-label={copied ? t("chat.copied") : t("chat.copy")}
        title={copied ? t("chat.copied") : t("chat.copy")}
      >
        {copied ? (
          <Check size={14} strokeWidth={2.4} aria-hidden />
        ) : (
          <Copy size={14} strokeWidth={2} aria-hidden />
        )}
      </button>
      {role === "assistant" ? (
        <button
          type="button"
          className="msg-action-btn"
          disabled={disabled || !onRegenerate}
          onClick={() => onRegenerate?.(messageId)}
          aria-label={t("chat.regenerate")}
          title={t("chat.regenerate")}
        >
          <RefreshCw size={14} strokeWidth={2} aria-hidden />
        </button>
      ) : (
        <button
          type="button"
          className="msg-action-btn"
          disabled={disabled || !onEdit}
          onClick={() => onEdit?.(messageId)}
          aria-label={t("chat.editResend")}
          title={t("chat.editResend")}
        >
          <Pencil size={14} strokeWidth={2} aria-hidden />
        </button>
      )}
      <button
        type="button"
        className="msg-action-btn msg-action-btn--danger"
        disabled={disabled || !onDelete}
        onClick={() => onDelete?.(messageId)}
        aria-label={t("chat.delete")}
        title={t("chat.delete")}
      >
        <Trash2 size={14} strokeWidth={2} aria-hidden />
      </button>
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
    </div>
  );
}

export default function ChatView({
  messages,
  input,
  attachments,
  streaming,
  streamPaused = false,
  sendBlocked = false,
  sendBlockedReason,
  displayPrefs,
  emptyMode,
  focusMessageId,
  onFocusConsumed,
  onInputChange,
  onAttachmentsChange,
  onSend,
  queuedFollowUps = [],
  onRemoveQueuedFollowUp,
  onUpdateQueuedFollowUpText,
  onMoveQueuedFollowUp,
  pendingInterrupts = [],
  onUiAction,
  onPauseStream,
  onResumeStream,
  onStopStream,
  onNewChat,
  onSkipAgentCreate,
  onPickWelcomePrompt,
  showThinkingControls = false,
  thinkingPrefs,
  onToggleThinking: _onToggleThinking,
  onThinkingLevelChange,
  agentId = null,
  modelId = null,
  onOpenMcpSettings,
  chatMode,
  onChatModeChange,
  onOpenContext,
  contextUsagePercent = null,
  contextUsage = null,
  contextWindow = 0,
  onRegenerateMessage,
  onEditUserMessage,
  dissolvingIds = [],
  onDeleteMessage,
  onBranchMessage,
  onSlashAction,
}: Props) {
  const { t } = useI18n();
  const dissolvingSet = useMemo(() => new Set(dissolvingIds), [dissolvingIds]);
  const bottomRef = useRef<HTMLDivElement>(null);
  const messageListRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const typedHintRef = useRef<HTMLSpanElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const modeMenuRef = useRef<HTMLDivElement>(null);
  const modeMenuPanelRef = useRef<HTMLDivElement>(null);
  const mcpWrapRef = useRef<HTMLDivElement>(null);
  const contextWrapRef = useRef<HTMLDivElement>(null);
  const [modeMenuOpen, setModeMenuOpen] = useState(false);
  const [queueOpen, setQueueOpen] = useState(true);
  const [editingQueueId, setEditingQueueId] = useState<string | null>(null);
  const [contextPopoverOpen, setContextPopoverOpen] = useState(false);
  const [mcpOpen, setMcpOpen] = useState(false);
  const [paletteKind, setPaletteKind] = useState<PaletteKind | null>(null);
  const [paletteQuery, setPaletteQuery] = useState("");
  const [paletteIndex, setPaletteIndex] = useState(0);
  const [triggerStart, setTriggerStart] = useState(0);
  const [agents, setAgents] = useState<AgentIconInfo[]>([]);
  const [activeAgentId, setActiveAgentId] = useState<string | null>(null);
  const [skills, setSkills] = useState<InstalledSkill[]>([]);
  const [mediaBaseDir, setMediaBaseDir] = useState<string | null>(null);
  /** 创建 Agent：发送校验失败时高亮的必填槽 index */
  const [agentCreateMissing, setAgentCreateMissing] = useState<number[]>([]);
  const { servers: mcpServers } = useMcpTools(agentId);
  const mcpHasEnabled = mcpServers.some((s) => s.enabled);

  const loadMentionSources = useCallback(async () => {
    try {
      const cfg = await invoke<{
        workspace_dir: string;
        active_agent_id: string;
        agents: (AgentIconInfo & { path?: string })[];
      }>("get_config");
      setAgents(cfg.agents ?? []);
      setActiveAgentId(cfg.active_agent_id ?? null);
      const scopedId = agentId ?? cfg.active_agent_id;
      const scoped = cfg.agents?.find((a) => a.id === scopedId);
      setMediaBaseDir(scoped?.path?.trim() || cfg.workspace_dir || null);
    } catch {
      setAgents([]);
      setActiveAgentId(null);
      setMediaBaseDir(null);
    }
    try {
      const list = await invoke<InstalledSkill[]>("list_installed_skills");
      setSkills((list ?? []).filter((s) => s.enabled));
    } catch {
      setSkills([]);
    }
  }, [agentId]);

  const activeAgent = useMemo(() => {
    if (!agents.length) return null;
    const id = agentId ?? activeAgentId;
    if (id) {
      return agents.find((a) => a.id === id) ?? agents[0] ?? null;
    }
    return agents[0] ?? null;
  }, [agents, agentId, activeAgentId]);

  const assistantHasCustomAvatar = Boolean(
    activeAgent &&
      (isAgentIconSrc(activeAgent.avatar) || isAgentIconSrc(activeAgent.emoji)),
  );

  useEffect(() => {
    void loadMentionSources();
  }, [loadMentionSources]);

  const welcomeHints = useMemo(
    () => [
      t("chat.welcomeHint.1"),
      t("chat.welcomeHint.2"),
      t("chat.welcomeHint.3"),
      t("chat.welcomeHint.4"),
      t("chat.welcomeHint.5"),
    ],
    [t],
  );
  const typingPlaceholderEnabled =
    emptyMode === "chat" &&
    !streaming &&
    pendingInterrupts.length === 0 &&
    input.length === 0 &&
    attachments.length === 0;
  useTypingPlaceholder(welcomeHints, typingPlaceholderEnabled, typedHintRef);

  useEffect(() => {
    if (!modeMenuOpen) return;
    const onDoc = (ev: MouseEvent) => {
      const target = ev.target as Node;
      if (modeMenuRef.current?.contains(target)) return;
      if (modeMenuPanelRef.current?.contains(target)) return;
      setModeMenuOpen(false);
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [modeMenuOpen]);

  const modeMenuStyle = useClampPopover({
    open: modeMenuOpen,
    anchorRef: modeMenuRef,
    popoverRef: modeMenuPanelRef,
    mode: "fixed",
    preferAlign: "start",
    placement: "above",
    gap: 8,
    maxHeightCap: 320,
    minMaxHeight: 96,
    sizeKey: chatMode,
  });

  const modeMeta = useMemo(() => {
    const map: Record<
      ChatInteractionMode,
      { label: string; Icon: typeof InfinityIcon }
    > = {
      agent: { label: t("chat.modeAgent"), Icon: InfinityIcon },
      plan: { label: t("chat.modePlan"), Icon: ListTree },
      ask: { label: t("chat.modeAsk"), Icon: MessageCircle },
      multitask: { label: t("chat.modeMultitask"), Icon: Layers2 },
    };
    return map;
  }, [t]);

  const thinkingItems: PaletteItem[] = useMemo(
    () => [
      {
        id: "off",
        level: "off",
        title: t("chat.thinkLevelOff"),
        description: t("chat.thinkLevelOffDesc"),
      },
      {
        id: "low",
        level: "low",
        title: t("chat.thinkLevelLow"),
        description: t("chat.thinkLevelLowDesc"),
      },
      {
        id: "high",
        level: "high",
        title: t("chat.thinkLevelHigh"),
        description: t("chat.thinkLevelHighDesc"),
      },
      {
        id: "max",
        level: "max",
        title: t("chat.thinkLevelMax"),
        description: t("chat.thinkLevelMaxDesc"),
      },
    ],
    [t],
  );

  const slashItems: PaletteItem[] = useMemo(() => {
    return buildSlashPaletteEntries(skills).map((e) => ({
      id: e.id,
      title: e.title,
      description: e.description ?? t(e.descKey),
      action: e.action,
      icon: e.icon,
      skillName: e.skillName,
      // Hermes：技能斜杠插入 /name，发送时再注入 SKILL.md
      insert:
        e.action === "insert_skill" && e.skillName
          ? `/${e.skillName} `
          : undefined,
    }));
  }, [skills, t]);

  const mentionItems: PaletteItem[] = useMemo(() => {
    const candidates = buildMentionCandidates({
      agents: agents
        .filter((a): a is AgentIconInfo & { id: string; name: string } =>
          Boolean(a.id && a.name),
        )
        .map((a) => ({ id: a.id, name: a.name })),
      skills,
      mcpServers: mcpServers.map((s) => ({
        id: s.id,
        name: s.name,
        description: s.description,
      })),
    });
    return candidates.map((c) => {
      const descKey =
        c.kind === "agent"
          ? "chat.mentionAgent"
          : c.kind === "skill"
            ? "chat.mentionSkill"
            : "chat.mentionMcp";
      const icon = c.kind === "agent" ? "◎" : c.kind === "skill" ? "✦" : "⬡";
      return {
        id: `mention-${c.kind}-${c.id}`,
        title: `@${c.name}`,
        description: c.description || t(descKey),
        insert: `@${c.name} `,
        icon,
        mentionKind: c.kind,
        action: "insert" as const,
      };
    });
  }, [agents, skills, mcpServers, t]);

  const activePaletteItems = useMemo(() => {
    if (paletteKind === "thinking") return thinkingItems;
    if (paletteKind === "slash") return slashItems;
    if (paletteKind === "mention") return mentionItems;
    return [];
  }, [paletteKind, thinkingItems, slashItems, mentionItems]);

  const filteredPaletteItems = useMemo(() => {
    if (paletteKind === "thinking") return activePaletteItems;
    const q = paletteQuery.trim().toLowerCase();
    if (!q) return activePaletteItems;
    return activePaletteItems.filter(
      (it) =>
        it.title.toLowerCase().includes(q) ||
        it.id.toLowerCase().includes(q) ||
        (it.description?.toLowerCase().includes(q) ?? false),
    );
  }, [activePaletteItems, paletteKind, paletteQuery]);

  const closePalette = useCallback(() => {
    setPaletteKind(null);
    setPaletteQuery("");
    setPaletteIndex(0);
  }, []);

  const openThinkingPalette = useCallback(() => {
    setMcpOpen(false);
    setModeMenuOpen(false);
    setContextPopoverOpen(false);
    setPaletteKind((k) => (k === "thinking" ? null : "thinking"));
    setPaletteQuery("");
    setPaletteIndex(
      Math.max(
        0,
        thinkingItems.findIndex((it) => it.level === thinkingPrefs.level),
      ),
    );
  }, [thinkingItems, thinkingPrefs.level]);

  const insertAtTrigger = useCallback(
    (insert: string, start: number, end: number) => {
      const next = `${input.slice(0, start)}${insert}${input.slice(end)}`;
      onInputChange(next);
      closePalette();
      requestAnimationFrame(() => {
        const el = textareaRef.current;
        if (!el) return;
        const caret = start + insert.length;
        el.focus();
        el.setSelectionRange(caret, caret);
      });
    },
    [input, onInputChange, closePalette],
  );

  const runSlashAction = useCallback(
    (action: SlashAction, args?: string, skillName?: string) => {
      if (action === "help") {
        onInputChange(t("chat.slashHelpInsert"));
        closePalette();
        return;
      }
      if (action === "insert_skill") {
        const name = skillName ?? args?.trim() ?? "";
        if (!name) return;
        // 调色板选技能：插入 /name 待用户补全任务；对齐 Hermes
        onInputChange(`/${name} `);
        closePalette();
        requestAnimationFrame(() => {
          const el = textareaRef.current;
          if (!el) return;
          const caret = el.value.length;
          el.focus();
          el.setSelectionRange(caret, caret);
        });
        return;
      }
      if (action === "new_chat") {
        closePalette();
        onNewChat();
        return;
      }
      closePalette();
      onSlashAction?.(action, args);
    },
    [closePalette, onInputChange, onNewChat, onSlashAction, t],
  );

  const tryHandleSlashSubmit = useCallback((): boolean => {
    const skillNames = skills.map((s) => s.name);
    const parsed = parseSlashInput(input, skillNames);
    if (!parsed) {
      if (input.trim().startsWith("/")) {
        // 可能是链式 /skill /skill2 task — 交给 send 解析
        const first = input.trim().slice(1).split(/\s/)[0] ?? "";
        const isSkill = skillNames.some(
          (n) => n.toLowerCase() === first.toLowerCase(),
        );
        if (isSkill || first.includes("/")) {
          return false;
        }
        const cmd = first;
        onInputChange(t("chat.slashUnknown", { cmd }));
        return true;
      }
      return false;
    }
    if (parsed.action === "insert_skill") {
      // 保持 /skill args 原样发送，由 App.resolveComposerTurn 注入 SKILL.md
      return false;
    }
    if (parsed.action === "help") {
      onInputChange(t("chat.slashHelpInsert"));
      return true;
    }
    onInputChange("");
    onSlashAction?.(parsed.action, parsed.args);
    return true;
  }, [input, skills, onInputChange, onSlashAction, t]);

  const applyPaletteItem = useCallback(
    (item: PaletteItem) => {
      if (paletteKind === "thinking" && item.level) {
        onThinkingLevelChange(item.level);
        closePalette();
        return;
      }
      if (paletteKind === "slash" && item.action && item.action !== "insert") {
        runSlashAction(
          item.action === "clear" ? "new_chat" : (item.action as SlashAction),
          undefined,
          item.skillName,
        );
        return;
      }
      const insert = item.insert ?? `${item.title} `;
      const end = textareaRef.current?.selectionStart ?? input.length;
      insertAtTrigger(insert, triggerStart, end);
    },
    [
      paletteKind,
      onThinkingLevelChange,
      closePalette,
      runSlashAction,
      insertAtTrigger,
      triggerStart,
      input.length,
    ],
  );

  const syncTriggerFromCaret = useCallback(
    (text: string, caret: number) => {
      if (paletteKind === "thinking") return;
      const hit = detectTrigger(text, caret);
      if (!hit) {
        if (paletteKind === "slash" || paletteKind === "mention") closePalette();
        return;
      }
      setPaletteKind(hit.kind);
      setPaletteQuery(hit.query);
      setTriggerStart(hit.start);
      setPaletteIndex(0);
    },
    [paletteKind, closePalette],
  );

  const onComposerKeyDown = (e: ReactKeyboardEvent<HTMLTextAreaElement>) => {
    if (paletteKind && filteredPaletteItems.length > 0) {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setPaletteIndex((i) => (i + 1) % filteredPaletteItems.length);
        return;
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        setPaletteIndex(
          (i) => (i - 1 + filteredPaletteItems.length) % filteredPaletteItems.length,
        );
        return;
      }
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        applyPaletteItem(filteredPaletteItems[paletteIndex] ?? filteredPaletteItems[0]);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        closePalette();
        return;
      }
      if (e.key === "Tab") {
        e.preventDefault();
        applyPaletteItem(filteredPaletteItems[paletteIndex] ?? filteredPaletteItems[0]);
        return;
      }
    }

    if (e.key === "Tab" && emptyMode === "agent") {
      const el = textareaRef.current;
      if (!el) return;
      const caret = el.selectionStart ?? 0;
      const slot = e.shiftKey
        ? prevEmptySlot(input, caret)
        : nextEmptySlot(input, caret);
      if (slot) {
        e.preventDefault();
        requestAnimationFrame(() => {
          el.focus();
          el.setSelectionRange(slot.innerStart, slot.innerEnd);
        });
      }
      return;
    }
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      if (tryHandleSlashSubmit()) return;
      trySubmitComposer();
    }
  };

  useEffect(() => {
    if (focusMessageId) return;
    bottomRef.current?.scrollIntoView({
      behavior: streaming ? "auto" : "smooth",
    });
  }, [messages, attachments, streaming, focusMessageId]);

  useEffect(() => {
    if (!focusMessageId) return;
    let raf = 0;
    let timer = 0;

    const focusEl = (el: HTMLElement) => {
      el.scrollIntoView({ behavior: "smooth", block: "center" });
      el.classList.add("is-focus-flash");
      timer = window.setTimeout(() => el.classList.remove("is-focus-flash"), 1600);
      onFocusConsumed?.();
    };

    const tryFocus = (retried: boolean) => {
      const el = document.getElementById(`msg-${focusMessageId}`);
      if (el) {
        focusEl(el);
        return;
      }
      if (
        messages.length > 0 &&
        !messages.some((m) => m.id === focusMessageId)
      ) {
        onFocusConsumed?.();
        return;
      }
      if (!retried) {
        raf = requestAnimationFrame(() => tryFocus(true));
      }
    };

    tryFocus(false);
    return () => {
      cancelAnimationFrame(raf);
      window.clearTimeout(timer);
    };
  }, [focusMessageId, messages, onFocusConsumed]);

  useEffect(() => {
    const el = textareaRef.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 120)}px`;
  }, [input]);

  const revokePreview = (att: ChatAttachment) => {
    if (att.previewUrl) URL.revokeObjectURL(att.previewUrl);
  };

  const addFiles = useCallback(
    async (files: FileList | File[]) => {
      const list = Array.from(files);
      if (!list.length) return;
      const room = MAX_ATTACHMENTS - attachments.length;
      if (room <= 0) return;
      const nextBatch = list.slice(0, room);
      const created = await Promise.all(nextBatch.map((f) => fileToAttachment(f)));
      onAttachmentsChange([...attachments, ...created]);
    },
    [attachments, onAttachmentsChange],
  );

  const addAttachments = useCallback(
    (created: ChatAttachment[]) => {
      if (!created.length) return;
      const room = MAX_ATTACHMENTS - attachments.length;
      if (room <= 0) return;
      onAttachmentsChange([...attachments, ...created.slice(0, room)]);
    },
    [attachments, onAttachmentsChange],
  );

  const addPaths = useCallback(
    async (paths: string[]) => {
      if (!paths.length || (streaming && chatMode === "multitask")) return;
      const created = await pathsToAttachments(paths);
      addAttachments(created);
    },
    [addAttachments, streaming, chatMode],
  );

  const attachMediaPath = useCallback(
    async (path: string) => {
      const att = await pathToAttachment(path);
      addAttachments([att]);
      if (!input.trim()) {
        onInputChange(t("media.quotePrompt"));
      }
      window.requestAnimationFrame(() => {
        textareaRef.current?.focus();
      });
    },
    [addAttachments, input, onInputChange, t],
  );

  const mediaAttachApi = useMemo(
    () => ({ attachMediaPath }),
    [attachMediaPath],
  );

  const [fileDragOver, setFileDragOver] = useState(false);
  const dragDepthRef = useRef(0);
  const tauriDropAtRef = useRef(0);

  useEffect(() => {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
      return;
    }
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void import("@tauri-apps/api/webview")
      .then(({ getCurrentWebview }) =>
        getCurrentWebview().onDragDropEvent((event) => {
          if (streaming && chatMode === "multitask") return;
          const kind = event.payload.type;
          if (kind === "enter" || kind === "over") {
            setFileDragOver(true);
            return;
          }
          if (kind === "leave") {
            setFileDragOver(false);
            return;
          }
          if (kind === "drop") {
            setFileDragOver(false);
            tauriDropAtRef.current = Date.now();
            void addPaths(event.payload.paths);
          }
        }),
      )
      .then((fn) => {
        if (cancelled) {
          fn();
          return;
        }
        unlisten = fn;
      })
      .catch(() => {
        // non-Tauri / older runtime
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [addPaths, streaming, chatMode]);

  const removeAttachment = (id: string) => {
    const target = attachments.find((a) => a.id === id);
    if (target) revokePreview(target);
    onAttachmentsChange(attachments.filter((a) => a.id !== id));
  };

  const onFileChange = async (e: ChangeEvent<HTMLInputElement>) => {
    if (e.target.files) await addFiles(e.target.files);
    e.target.value = "";
  };

  const onDragEnter = (e: DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    if (streaming && chatMode === "multitask") return;
    dragDepthRef.current += 1;
    if (e.dataTransfer?.types?.includes("Files")) {
      setFileDragOver(true);
    }
  };

  const onDragLeave = (e: DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    dragDepthRef.current = Math.max(0, dragDepthRef.current - 1);
    if (dragDepthRef.current === 0) setFileDragOver(false);
  };

  const onDrop = async (e: DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    dragDepthRef.current = 0;
    setFileDragOver(false);
    if (streaming && chatMode === "multitask") return;
    // Tauri 原生 drop 已处理时跳过，避免重复添加
    if (Date.now() - tauriDropAtRef.current < 500) return;
    if (e.dataTransfer.files?.length) {
      await addFiles(e.dataTransfer.files);
      return;
    }
    const paths = pathsFromDataTransfer(e.dataTransfer);
    if (paths.length) await addPaths(paths);
  };

  const onPaste = async (e: {
    clipboardData: DataTransfer | null;
    preventDefault: () => void;
  }) => {
    if (streaming && chatMode === "multitask") return;
    const list = e.clipboardData?.files;
    if (list && list.length > 0) {
      e.preventDefault();
      await addFiles(list);
      return;
    }
    const items = e.clipboardData?.items;
    if (items) {
      const files: File[] = [];
      for (let i = 0; i < items.length; i += 1) {
        const item = items[i];
        if (item.kind === "file") {
          const f = item.getAsFile();
          if (f) files.push(f);
        }
      }
      if (files.length) {
        e.preventDefault();
        await addFiles(files);
        return;
      }
    }

    const text = e.clipboardData?.getData("text/plain") ?? "";
    const pathList = pathsFromClipboardText(text);
    if (pathList.length) {
      e.preventDefault();
      const created = await pathsToAttachments(pathList);
      if (created.length) {
        addAttachments(created);
        return;
      }
    }

    // 系统文件剪贴板（复制媒体后）在 WKWebView 里常不进 clipboardData.files
    // 先拦截，再读 OS 剪贴板 / 图片像素；都没有则回填普通文本
    e.preventDefault();
    const fromOs = await attachmentsFromOsClipboard();
    if (fromOs.length) {
      addAttachments(fromOs);
      return;
    }
    const fromRead = await filesFromClipboardRead();
    if (fromRead.length) {
      await addFiles(fromRead);
      return;
    }
    if (text) {
      const el = textareaRef.current;
      if (el) {
        const start = el.selectionStart ?? el.value.length;
        const end = el.selectionEnd ?? start;
        const next = el.value.slice(0, start) + text + el.value.slice(end);
        onInputChange(next);
        const caret = start + text.length;
        window.requestAnimationFrame(() => {
          el.setSelectionRange(caret, caret);
        });
      }
    }
  };

  const interruptBlocked = pendingInterrupts.length > 0;
  const queueEnabled = chatMode !== "multitask";
  const canQueueWhileStreaming = streaming && queueEnabled;
  const canSend =
    !interruptBlocked &&
    !sendBlocked &&
    (input.trim().length > 0 || attachments.length > 0) &&
    (!streaming || canQueueWhileStreaming);

  const slotMirrorRef = useRef<HTMLDivElement>(null);
  const agentTemplateSegments = useMemo(
    () => (emptyMode === "agent" ? listTemplateSegments(input) : []),
    [emptyMode, input],
  );
  const agentCreateMissingSet = useMemo(
    () => new Set(agentCreateMissing),
    [agentCreateMissing],
  );

  useEffect(() => {
    setAgentCreateMissing((prev) => {
      if (emptyMode !== "agent" || prev.length === 0) {
        return prev.length === 0 ? prev : [];
      }
      const still = prepareAgentCreateSend(input).missingRequired.map((s) => s.index);
      if (
        still.length === prev.length &&
        still.every((idx, n) => idx === prev[n])
      ) {
        return prev;
      }
      return still;
    });
  }, [emptyMode, input]);

  const trySubmitComposer = () => {
    if (!canSend) return;
    if (emptyMode === "agent") {
      const prep = prepareAgentCreateSend(input);
      if (!prep.ok) {
        setAgentCreateMissing(prep.missingRequired.map((s) => s.index));
        const first = prep.missingRequired[0];
        const el = textareaRef.current;
        if (first && el) {
          requestAnimationFrame(() => {
            el.focus();
            el.setSelectionRange(first.innerStart, first.innerEnd);
          });
        }
        return;
      }
      setAgentCreateMissing([]);
      onSend({ text: prep.sanitized });
      return;
    }
    onSend();
  };

  useEffect(() => {
    if (emptyMode !== "agent") return;
    const el = textareaRef.current;
    if (!el) return;
    const slot = nextEmptySlot(input, -1);
    if (!slot) return;
    requestAnimationFrame(() => {
      el.focus();
      el.setSelectionRange(slot.innerStart, slot.innerEnd);
    });
    // 仅在进入创建模式时定位首个空槽
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [emptyMode]);

  const syncSlotMirrorScroll = () => {
    const ta = textareaRef.current;
    const mirror = slotMirrorRef.current;
    if (!ta || !mirror) return;
    mirror.scrollTop = ta.scrollTop;
    mirror.scrollLeft = ta.scrollLeft;
  };

  const composerPlaceholder = streaming
    ? queueEnabled
      ? t("chat.placeholderStreaming")
      : t("chat.placeholderStreamingBusy")
    : sendBlocked && sendBlockedReason
      ? sendBlockedReason
      : interruptBlocked
        ? t("chat.interrupt.pending")
        : emptyMode === "chat"
          ? ""
          : attachments.length
            ? t("chat.placeholderWithAttach")
            : t("chat.placeholder");

  return (
    <ChatMediaAttachProvider value={mediaAttachApi}>
    <section
      className={`chat-pane ${fileDragOver ? "is-file-dragover" : ""}`.trim()}
      onDragEnter={onDragEnter}
      onDragLeave={onDragLeave}
      onDragOver={(e) => {
        e.preventDefault();
        e.stopPropagation();
      }}
      onDrop={(e) => void onDrop(e)}
    >
      {emptyMode === "chat" ? (
        <ChatWelcome onPickCard={onPickWelcomePrompt} />
      ) : emptyMode === "agent" ? (
          <AgentCreateGuide
            onSkip={onSkipAgentCreate}
            previewName={firstSlotValue(input)}
          />
      ) : (
        <div className="message-list-wrap">
          <div className="message-list" ref={messageListRef}>
            {messages.map((m, index) => {
              const isStreamingBubble =
                streaming &&
                m.role === "assistant" &&
                !m.error &&
                index === messages.length - 1;
              const reasoningActive = Boolean(
                isStreamingBubble && m.reasoning && !m.content,
              );
              const dissolving = dissolvingSet.has(m.id);
              const dissolveStagger = dissolving
                ? Math.max(0, dissolvingIds.indexOf(m.id))
                : 0;
              return (
                <div
                  key={m.id}
                  id={`msg-${m.id}`}
                  data-msg-id={m.id}
                  className={`msg-row ${m.role === "user" ? "user" : "assistant"}${
                    dissolving ? " is-dissolving" : ""
                  }`}
                >
                  {dissolving ? (
                    <MsgDissolveOverlay
                      messageId={m.id}
                      text={`${m.content || ""}${m.reasoning || ""}`}
                      staggerIndex={dissolveStagger}
                    />
                  ) : null}
                  {m.role === "assistant" && (
                    <div
                      className={`avatar ${m.error ? "error" : ""}${
                        !m.error && !assistantHasCustomAvatar && modelId
                          ? " is-model"
                          : ""
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
                      } ${isStreamingBubble && (m.content || m.reasoning) ? "is-streaming" : ""}`}
                    >
                      {m.attachments && m.attachments.length > 0 && (
                        <MessageAttachments items={m.attachments} />
                      )}
                      {(() => {
                        type Step = {
                          key: string;
                          kind: MsgTimelineKind;
                          active?: boolean;
                          node: ReactNode;
                        };
                        const steps: Step[] = [];
                        const pushActivity = (act: ChatActivity) => {
                          if (!isActivityVisible(act.kind, displayPrefs)) return;
                          steps.push({
                            key: `act-${act.id}`,
                            kind: act.kind,
                            active: act.status === "running",
                            node: (
                              <MsgActivity
                                activity={act}
                                defaultOpen={false}
                                showTimestamp={displayPrefs.showTimestamps}
                                mediaBaseDir={mediaBaseDir}
                              />
                            ),
                          });
                        };
                        const pushSurface = (
                          surface: NonNullable<ChatMessage["uiSurfaces"]>[number],
                        ) => {
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
                            ) : surface.interrupts && surface.interrupts.length > 0 ? (
                                <A2UIRenderer
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

                        if (m.segments && m.segments.length > 0) {
                          const displaySegs =
                            coalesceReasoningSegments(m.segments) ?? m.segments;
                          for (const seg of displaySegs) {
                            if (seg.type === "reasoning") {
                              const openReasoning =
                                seg.durationSec == null || seg.durationSec <= 0;
                              const active = Boolean(
                                isStreamingBubble && openReasoning,
                              );
                              steps.push({
                                key: seg.id,
                                kind: "reasoning",
                                active,
                                node: (
                                  <MsgReasoning
                                    reasoning={seg.text}
                                    active={active}
                                    durationSec={seg.durationSec}
                                    startedAtMs={active ? seg.at : undefined}
                                  />
                                ),
                              });
                              continue;
                            }
                            if (seg.type === "activity") {
                              const act = m.activities?.find(
                                (a) => a.id === seg.id,
                              );
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
                                  durationSec={m.reasoningDurationSec}
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
                        }

                        const showLoaderAlone =
                          isStreamingBubble &&
                          !m.content &&
                          !m.reasoning &&
                          !m.attachments?.length &&
                          !m.uiSurfaces?.length &&
                          !(
                            m.activities?.length &&
                            displayPrefs.verbosity !== "compact"
                          );
                        const hasProcess = steps.length > 0;

                        if (m.content) {
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
                                {isStreamingBubble ? <MsgStreamLoader /> : null}
                              </>
                            ),
                          });
                        } else if (isStreamingBubble && hasProcess) {
                          steps.push({
                            key: `gen-${m.id}`,
                            kind: "generating",
                            active: true,
                            node: <MsgStreamLoader />,
                          });
                        } else if (showLoaderAlone) {
                          steps.push({
                            key: `gen-${m.id}`,
                            kind: "generating",
                            active: true,
                            node: <MsgStreamLoader alone />,
                          });
                        } else if (isStreamingBubble && !hasProcess) {
                          steps.push({
                            key: `gen-${m.id}`,
                            kind: "generating",
                            active: true,
                            node: <MsgStreamLoader />,
                          });
                        }

                        if (steps.length === 0) return null;

                        if (!hasProcess) {
                          return <>{steps.map((step) => (
                            <div key={step.key}>{step.node}</div>
                          ))}</>;
                        }

                        return (
                          <MsgTimeline>
                            {steps.map((step, i) => (
                              <MsgTimelineStep
                                key={step.key}
                                kind={step.kind}
                                active={step.active}
                                isLast={i === steps.length - 1}
                              >
                                {step.node}
                              </MsgTimelineStep>
                            ))}
                          </MsgTimeline>
                        );
                      })()}
                      {displayPrefs.showTimestamps && m.createdAt ? (
                        <div className="msg-timestamp">
                          {new Date(m.createdAt).toLocaleTimeString()}
                        </div>
                      ) : null}
                      {m.role === "assistant" &&
                      !isStreamingBubble &&
                      (m.usage || m.generationDurationSec) ? (
                        <MessageTokenStats
                          usage={m.usage}
                          tokensPerSec={m.tokensPerSec}
                          generationDurationSec={m.generationDurationSec}
                        />
                      ) : null}
                    </div>
                    {!isStreamingBubble &&
                    !dissolving &&
                    m.id !== "welcome" &&
                    (m.role === "user" || m.role === "assistant") ? (
                      <MessageActions
                        messageId={m.id}
                        content={m.content}
                        role={m.role}
                        disabled={streaming || dissolvingIds.length > 0}
                        onRegenerate={
                          m.role === "assistant" ? onRegenerateMessage : undefined
                        }
                        onEdit={
                          m.role === "user" ? onEditUserMessage : undefined
                        }
                        onDelete={onDeleteMessage}
                        onBranch={onBranchMessage}
                      />
                    ) : null}
                  </div>
                </div>
              );
            })}
            <div ref={bottomRef} />
          </div>
          <ChatMessageNav
            messages={messages}
            listRef={messageListRef}
            bottomRef={bottomRef}
          />
        </div>
      )}

      <form
        className="composer-shell"
        onSubmit={(e) => {
          e.preventDefault();
          if (tryHandleSlashSubmit()) return;
          trySubmitComposer();
        }}
      >
        {queuedFollowUps.length > 0 && (
          <div className="composer-queue" aria-label={t("chat.queue.title", { count: String(queuedFollowUps.length) })}>
            <button
              type="button"
              className="composer-queue-toggle"
              aria-expanded={queueOpen}
              onClick={() => setQueueOpen((o) => !o)}
            >
              <ChevronDown
                size={14}
                strokeWidth={2.2}
                className={queueOpen ? "is-open" : ""}
                aria-hidden
              />
              <span>
                {t("chat.queue.title", { count: String(queuedFollowUps.length) })}
              </span>
            </button>
            {queueOpen && (
              <ul className="composer-queue-list">
                {queuedFollowUps.map((item, index) => (
                  <li key={item.id} className="composer-queue-item">
                    <span className="composer-queue-dot" aria-hidden />
                    {editingQueueId === item.id ? (
                      <input
                        className="composer-queue-edit"
                        value={item.text}
                        autoFocus
                        onChange={(e) =>
                          onUpdateQueuedFollowUpText?.(item.id, e.target.value)
                        }
                        onBlur={() => setEditingQueueId(null)}
                        onKeyDown={(e) => {
                          if (e.key === "Enter") {
                            e.preventDefault();
                            setEditingQueueId(null);
                          }
                          if (e.key === "Escape") setEditingQueueId(null);
                        }}
                      />
                    ) : (
                      <span className="composer-queue-text">
                        {item.text.trim() || t("chat.queue.emptyText")}
                        {item.attachments.length > 0
                          ? ` · ${item.attachments.length}`
                          : ""}
                      </span>
                    )}
                    <span className="composer-queue-actions">
                      <button
                        type="button"
                        className="composer-queue-btn"
                        title={t("chat.queue.edit")}
                        aria-label={t("chat.queue.edit")}
                        onClick={() => setEditingQueueId(item.id)}
                      >
                        <Pencil size={13} strokeWidth={2.2} />
                      </button>
                      <button
                        type="button"
                        className="composer-queue-btn"
                        title={t("chat.queue.moveUp")}
                        aria-label={t("chat.queue.moveUp")}
                        disabled={index === 0}
                        onClick={() => onMoveQueuedFollowUp?.(item.id, -1)}
                      >
                        <ArrowUp size={13} strokeWidth={2.2} />
                      </button>
                      <button
                        type="button"
                        className="composer-queue-btn"
                        title={t("chat.queue.moveDown")}
                        aria-label={t("chat.queue.moveDown")}
                        disabled={index === queuedFollowUps.length - 1}
                        onClick={() => onMoveQueuedFollowUp?.(item.id, 1)}
                      >
                        <ArrowDown size={13} strokeWidth={2.2} />
                      </button>
                      <button
                        type="button"
                        className="composer-queue-btn"
                        title={t("chat.queue.remove")}
                        aria-label={t("chat.queue.remove")}
                        onClick={() => onRemoveQueuedFollowUp?.(item.id)}
                      >
                        <Trash2 size={13} strokeWidth={2.2} />
                      </button>
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </div>
        )}

        {attachments.length > 0 && (
          <div className="composer-previews">
            {attachments.map((att) => (
              <div key={att.id} className="composer-preview" data-kind={att.kind}>
                {att.kind === "image" && att.previewUrl ? (
                  <img src={att.previewUrl} alt={att.name} />
                ) : att.kind === "video" && att.previewUrl ? (
                  <video src={att.previewUrl} muted />
                ) : (
                  <span className="composer-preview-icon" data-kind={att.kind}>
                    <AttachmentGlyph kind={att.kind} />
                  </span>
                )}
                <div className="composer-preview-meta">
                  <span className="composer-preview-name">{att.name}</span>
                  <span className="composer-preview-size">{formatSize(att.size)}</span>
                </div>
                <button
                  type="button"
                  className="composer-preview-remove"
                  onClick={() => removeAttachment(att.id)}
                  aria-label={t("chat.removeAttachment")}
                  title={t("chat.removeAttachment")}
                  disabled={streaming}
                >
                  ×
                </button>
              </div>
            ))}
          </div>
        )}

        {paletteKind ? (
          <ComposerPalette
            kind={paletteKind}
            items={activePaletteItems}
            query={paletteQuery}
            activeIndex={paletteIndex}
            selectedId={
              paletteKind === "thinking" ? thinkingPrefs.level : null
            }
            onHover={setPaletteIndex}
            onSelect={applyPaletteItem}
            onClose={closePalette}
          />
        ) : null}

        <div
          className={`composer composer--stacked ${fileDragOver ? "is-file-dragover" : ""}`.trim()}
        >
          <input
            ref={fileInputRef}
            type="file"
            className="composer-file-input"
            multiple
            accept="image/*,video/*,audio/*,.pdf,.txt,.md,.json,.csv,.doc,.docx,.xls,.xlsx,.ppt,.pptx,.zip,.rs,.ts,.tsx,.js,.py"
            onChange={(e) => void onFileChange(e)}
            disabled={(streaming && chatMode === "multitask") || attachments.length >= MAX_ATTACHMENTS}
          />
          {fileDragOver ? (
            <div className="composer-drop-hint" aria-live="polite">
              {t("chat.dropFilesHint")}
            </div>
          ) : null}
          {emptyMode === "agent" && agentCreateMissing.length > 0 ? (
            <p className="composer-agent-validate-hint" role="alert">
              {t("chat.agentCreateNeedRequired")}
            </p>
          ) : null}
          <div className={`composer-input-wrap ${emptyMode === "agent" ? "is-agent-template" : ""}`.trim()}>
            <div
              className="composer-typed-hint"
              hidden={!typingPlaceholderEnabled}
              aria-hidden
            >
              <span ref={typedHintRef} className="composer-typed-text" />
              <span className="composer-typed-caret" />
            </div>
            {emptyMode === "agent" ? (
              <div ref={slotMirrorRef} className="composer-slot-mirror" aria-hidden>
                {agentTemplateSegments.map((seg, i) =>
                  seg.type === "text" ? (
                    <span key={`t-${i}`}>{seg.value}</span>
                  ) : (
                    <span
                      key={`s-${i}`}
                      className={[
                        "composer-slot-chip",
                        seg.empty ? "is-empty" : "is-filled",
                        agentCreateMissingSet.has(seg.index) ? "is-invalid" : "",
                      ]
                        .filter(Boolean)
                        .join(" ")}
                    >
                      {seg.open}
                      {seg.value || "\u00a0"}
                      {seg.close}
                    </span>
                  ),
                )}
              </div>
            ) : null}
            <textarea
              ref={textareaRef}
              className={`composer-input ${emptyMode === "agent" ? "is-slot-highlight" : ""}`.trim()}
              value={input}
              rows={2}
              onChange={(e) => {
                const v = e.target.value;
                onInputChange(v);
                syncTriggerFromCaret(v, e.target.selectionStart ?? v.length);
              }}
              onScroll={syncSlotMirrorScroll}
              onSelect={(e) => {
                const el = e.currentTarget;
                syncTriggerFromCaret(el.value, el.selectionStart ?? 0);
              }}
              onPaste={(e) => void onPaste(e)}
              onClick={() => {
                const el = textareaRef.current;
                if (!el) return;
                if (emptyMode === "agent") {
                  const caret = el.selectionStart ?? 0;
                  const slot = findSlotAt(input, caret);
                  if (slot) {
                    el.setSelectionRange(slot.innerStart, slot.innerEnd);
                    return;
                  }
                }
                syncTriggerFromCaret(el.value, el.selectionStart ?? 0);
              }}
              onKeyDown={onComposerKeyDown}
              placeholder={composerPlaceholder}
              aria-label={
                emptyMode === "agent"
                  ? t("chat.agentGuideComposerAria")
                  : emptyMode === "chat"
                    ? t("chat.welcomePlaceholder")
                    : composerPlaceholder || t("chat.placeholder")
              }
              disabled={streaming || interruptBlocked}
              autoFocus
            />
          </div>
          <div className="composer-bar">
            <div className="composer-bar-left">
              <div className="composer-mode" ref={modeMenuRef}>
                <button
                  type="button"
                  className={`composer-mode-pill ${modeMenuOpen ? "is-open" : ""}`}
                  disabled={streaming}
                  aria-haspopup="listbox"
                  aria-expanded={modeMenuOpen}
                  aria-label={t("chat.modeMenu")}
                  title={t("chat.modeMenu")}
                  onClick={() => {
                    setMcpOpen(false);
                    setContextPopoverOpen(false);
                    setModeMenuOpen((o) => !o);
                  }}
                >
                  {(() => {
                    const Meta = modeMeta[chatMode];
                    const Icon = Meta.Icon;
                    return (
                      <>
                        <Icon size={15} strokeWidth={2.2} />
                        <span>{Meta.label}</span>
                        <ChevronDown size={14} strokeWidth={2} />
                      </>
                    );
                  })()}
                </button>
                {modeMenuOpen && typeof document !== "undefined"
                  ? createPortal(
                      <div
                        ref={modeMenuPanelRef}
                        className="composer-mode-menu"
                        role="listbox"
                        style={modeMenuStyle ?? { visibility: "hidden" }}
                      >
                        {CHAT_MODES.map((mode) => {
                          const Meta = modeMeta[mode];
                          const Icon = Meta.Icon;
                          const selected = mode === chatMode;
                          return (
                            <button
                              key={mode}
                              type="button"
                              role="option"
                              aria-selected={selected}
                              className={`composer-mode-item ${selected ? "is-selected" : ""}`}
                              onClick={() => {
                                onChatModeChange(mode);
                                setModeMenuOpen(false);
                              }}
                            >
                              <Icon size={16} strokeWidth={2} />
                              <span>{Meta.label}</span>
                              {selected ? (
                                <Check size={14} strokeWidth={2.4} />
                              ) : null}
                            </button>
                          );
                        })}
                      </div>,
                      document.body,
                    )
                  : null}
              </div>

              {showThinkingControls ? (
                <button
                  type="button"
                  className={`composer-mode-pill composer-mode-pill--ghost ${thinkingPrefs.level !== "off" ? "is-on" : ""
                    } ${paletteKind === "thinking" ? "is-open" : ""}`}
                  onClick={openThinkingPalette}
                  disabled={streaming}
                  title={t("chat.thinkingLength")}
                  aria-label={t("chat.thinkingLength")}
                  aria-pressed={thinkingPrefs.level !== "off"}
                >
                  <Lightbulb size={14} strokeWidth={2} />
                  <span>
                    {thinkingPrefs.level === "off"
                      ? t("chat.thinkLevelOff")
                      : thinkingPrefs.level === "low"
                        ? t("chat.thinkLevelLow")
                        : thinkingPrefs.level === "max"
                          ? t("chat.thinkLevelMax")
                          : t("chat.thinkLevelHigh")}
                  </span>
                  <ChevronDown size={13} strokeWidth={2} />
                </button>
              ) : null}

              <div className="composer-mcp-wrap" ref={mcpWrapRef}>
                <button
                  type="button"
                  className={`composer-icon-btn ${mcpOpen ? "is-open" : ""} ${
                    mcpHasEnabled ? "has-dot" : ""
                  }`}
                  disabled={streaming}
                  title={t("chat.mcpMenu")}
                  aria-label={t("chat.mcpMenu")}
                  aria-expanded={mcpOpen}
                  onClick={() => {
                    setModeMenuOpen(false);
                    setPaletteKind(null);
                    setContextPopoverOpen(false);
                    setMcpOpen((v) => !v);
                  }}
                >
                  <McpIcon size={16} />
                </button>
                <ComposerMcpMenu
                  open={mcpOpen}
                  anchorRef={mcpWrapRef}
                  agentId={agentId}
                  onClose={() => setMcpOpen(false)}
                  onOpenSettings={() => onOpenMcpSettings?.()}
                />
              </div>

              <button
                type="button"
                className={`composer-icon-btn ${paletteKind === "mention" ? "is-open" : ""
                  }`}
                disabled={streaming}
                title={t("chat.mentionTitle")}
                aria-label={t("chat.mentionTitle")}
                onClick={() => {
                  setMcpOpen(false);
                  setContextPopoverOpen(false);
                  const el = textareaRef.current;
                  const caret = el?.selectionStart ?? input.length;
                  const next = `${input.slice(0, caret)}@${input.slice(caret)}`;
                  onInputChange(next);
                  setTriggerStart(caret);
                  setPaletteKind("mention");
                  setPaletteQuery("");
                  setPaletteIndex(0);
                  requestAnimationFrame(() => {
                    el?.focus();
                    el?.setSelectionRange(caret + 1, caret + 1);
                  });
                }}
              >
                <AtSign size={17} strokeWidth={2} />
              </button>
              <button
                type="button"
                className={`composer-icon-btn ${paletteKind === "slash" ? "is-open" : ""
                  }`}
                disabled={streaming}
                title={t("chat.slashTitle")}
                aria-label={t("chat.slashTitle")}
                onClick={() => {
                  setMcpOpen(false);
                  setContextPopoverOpen(false);
                  const el = textareaRef.current;
                  const caret = el?.selectionStart ?? input.length;
                  const next = `${input.slice(0, caret)}/${input.slice(caret)}`;
                  onInputChange(next);
                  setTriggerStart(caret);
                  setPaletteKind("slash");
                  setPaletteQuery("");
                  setPaletteIndex(0);
                  requestAnimationFrame(() => {
                    el?.focus();
                    el?.setSelectionRange(caret + 1, caret + 1);
                  });
                }}
              >
                <Slash size={17} strokeWidth={2} />
              </button>
            </div>

            <div className="composer-bar-right">
              <div className="composer-context-wrap" ref={contextWrapRef}>
                <button
                  type="button"
                  className={`composer-icon-btn composer-context-btn ${
                    contextPopoverOpen ? "is-open" : ""
                  }`}
                  disabled={streaming}
                  title={
                    contextUsagePercent != null
                      ? `${t("chat.contextUsage")} · ${contextUsagePercent}%`
                      : t("chat.contextUsageHint")
                  }
                  aria-label={t("chat.contextUsage")}
                  aria-expanded={contextPopoverOpen}
                  onClick={() => {
                    setModeMenuOpen(false);
                    setMcpOpen(false);
                    setPaletteKind(null);
                    setContextPopoverOpen((v) => !v);
                  }}
                >
                  <ChartPie size={17} strokeWidth={2} />
                  {contextUsagePercent != null ? (
                    <span className="composer-context-pct">
                      {contextUsagePercent}%
                    </span>
                  ) : null}
                </button>
                <ContextUsagePopover
                  open={contextPopoverOpen}
                  snapshot={contextUsage}
                  windowTokens={contextWindow}
                  containRef={contextWrapRef}
                  onClose={() => setContextPopoverOpen(false)}
                  onViewDetails={() => {
                    onOpenContext();
                  }}
                />
              </div>
              <button
                type="button"
                className="composer-icon-btn"
                onClick={() => fileInputRef.current?.click()}
                disabled={(streaming && chatMode === "multitask") || attachments.length >= MAX_ATTACHMENTS}
                title={t("chat.attach")}
                aria-label={t("chat.attach")}
              >
                <Paperclip size={17} strokeWidth={2} />
              </button>
              {streaming ? (
                <>
                  {streamPaused ? (
                    <button
                      type="button"
                      className="composer-icon-btn"
                      onClick={() => onResumeStream?.()}
                      title={t("chat.streamResume")}
                      aria-label={t("chat.streamResume")}
                    >
                      <Play size={17} strokeWidth={2.2} />
                    </button>
                  ) : (
                    <button
                      type="button"
                      className="composer-icon-btn"
                      onClick={() => onPauseStream?.()}
                      title={t("chat.streamPause")}
                      aria-label={t("chat.streamPause")}
                    >
                      <Pause size={17} strokeWidth={2.2} />
                    </button>
                  )}
                  <button
                    type="button"
                    className="send-btn send-btn--round send-btn--stop"
                    onClick={() => onStopStream?.()}
                    aria-label={t("chat.streamStop")}
                    title={t("chat.streamStop")}
                  >
                    <Square size={14} strokeWidth={2.4} fill="currentColor" />
                  </button>
                </>
              ) : (
                <button
                  className="send-btn send-btn--round"
                  type="submit"
                  disabled={!canSend}
                  aria-label={t("chat.send")}
                  title={t("chat.send")}
                >
                    <SendHorizontal size={17} strokeWidth={2.2} />
                  </button>
              )}
            </div>
          </div>
        </div>
      </form>
    </section>
    </ChatMediaAttachProvider>
  );
}
