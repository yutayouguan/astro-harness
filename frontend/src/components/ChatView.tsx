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
} from "react";
import { invoke } from "@tauri-apps/api/core";
import {
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
} from "../hooks/useChatDisplayPrefs";
import { useI18n } from "../i18n/LocaleContext";
import {
  findSlotAt,
  firstSlotValue,
  nextEmptySlot,
  prevEmptySlot,
} from "../lib/agentCreateTemplate";
import {
  CHAT_MODES,
  type ChatInteractionMode,
} from "../lib/chatMode";
import type { ChatThinkingPrefs, ThinkingLevel } from "../lib/thinkingPrefs";
import type {
  ChatActivity,
  ChatAttachment,
  ChatAttachmentKind,
  ChatEmptyMode,
  ChatMessage,
  InstalledSkill,
  MessageTokenUsage,
  PendingInterrupt,
} from "../types";
import { AgentCreateGuide } from "./AgentCreateGuide";
import ChatMessageNav from "./ChatMessageNav";
import { ChatMarkdown } from "./ChatMarkdown";
import { ChatWelcome } from "./ChatWelcome";
import {
  ComposerPalette,
  type PaletteItem,
  type PaletteKind,
} from "./ComposerPalette";
import ComposerMcpMenu from "./ComposerMcpMenu";
import McpIcon from "./McpIcon";
import MsgActivity from "./MsgActivity";
import MsgDissolveOverlay from "./MsgDissolveOverlay";
import MsgReasoning from "./MsgReasoning";
import MsgStreamLoader from "./MsgStreamLoader";
import { useMcpTools } from "../hooks/useMcpTools";
import A2UIRenderer from "../a2ui/A2UIRenderer";
import { formatElapsedSec } from "../lib/elapsedSec";
import {
  buildMentionCandidates,
  buildSlashPaletteEntries,
  parseSlashInput,
  type SlashAction,
} from "../lib/composerCommands";

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
  const hasDuration =
    generationDurationSec != null && generationDurationSec > 0;
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
  /** 打开 Tools 面板 MCP tab */
  onOpenMcpSettings?: () => void;
  /** Agent / Plan / Ask / MultiTask */
  chatMode: ChatInteractionMode;
  onChatModeChange: (mode: ChatInteractionMode) => void;
  /** 打开右侧上下文面板 */
  onOpenContext: () => void;
  /** 简易上下文占用 0–100，用于按钮提示 */
  contextUsagePercent?: number | null;
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

/** 工具/技能等活动卡片列表（直接展示，无外层折叠包裹） */
function ActivityCards({
  items,
  prefs,
  showTimestamps,
}: {
  items: ChatActivity[];
  prefs: ChatDisplayPrefs;
  showTimestamps: boolean;
}) {
  const visible = items.filter((a) => isActivityVisible(a.kind, prefs));
  if (!visible.length) return null;
  return (
    <div className="msg-activities">
      {visible.map((a) => (
        <MsgActivity
          key={a.id}
          activity={a}
          defaultOpen={false}
          showTimestamp={showTimestamps}
        />
      ))}
    </div>
  );
}

export default function ChatView({
  messages,
  input,
  attachments,
  streaming,
  streamPaused = false,
  displayPrefs,
  emptyMode,
  focusMessageId,
  onFocusConsumed,
  onInputChange,
  onAttachmentsChange,
  onSend,
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
  onOpenMcpSettings,
  chatMode,
  onChatModeChange,
  onOpenContext,
  contextUsagePercent = null,
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
  const fileInputRef = useRef<HTMLInputElement>(null);
  const modeMenuRef = useRef<HTMLDivElement>(null);
  const [modeMenuOpen, setModeMenuOpen] = useState(false);
  const [mcpOpen, setMcpOpen] = useState(false);
  const [paletteKind, setPaletteKind] = useState<PaletteKind | null>(null);
  const [paletteQuery, setPaletteQuery] = useState("");
  const [paletteIndex, setPaletteIndex] = useState(0);
  const [triggerStart, setTriggerStart] = useState(0);
  const [agents, setAgents] = useState<{ id: string; name: string }[]>([]);
  const [skills, setSkills] = useState<InstalledSkill[]>([]);
  const { servers: mcpServers } = useMcpTools(agentId);
  const mcpHasEnabled = mcpServers.some((s) => s.enabled);

  const loadMentionSources = useCallback(async () => {
    try {
      const cfg = await invoke<{
        agents: { id: string; name: string }[];
      }>("get_config");
      setAgents(cfg.agents ?? []);
    } catch {
      setAgents([]);
    }
    try {
      const list = await invoke<InstalledSkill[]>("list_installed_skills");
      setSkills((list ?? []).filter((s) => s.enabled));
    } catch {
      setSkills([]);
    }
  }, []);

  useEffect(() => {
    void loadMentionSources();
  }, [loadMentionSources]);

  useEffect(() => {
    if (!modeMenuOpen) return;
    const onDoc = (ev: MouseEvent) => {
      if (!modeMenuRef.current?.contains(ev.target as Node)) {
        setModeMenuOpen(false);
      }
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [modeMenuOpen]);

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
      agents,
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
      if (canSend) onSend();
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

  const removeAttachment = (id: string) => {
    const target = attachments.find((a) => a.id === id);
    if (target) revokePreview(target);
    onAttachmentsChange(attachments.filter((a) => a.id !== id));
  };

  const onFileChange = async (e: ChangeEvent<HTMLInputElement>) => {
    if (e.target.files) await addFiles(e.target.files);
    e.target.value = "";
  };

  const onDrop = async (e: DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    if (streaming) return;
    if (e.dataTransfer.files?.length) await addFiles(e.dataTransfer.files);
  };

  const onPaste = async (e: {
    clipboardData: DataTransfer | null;
    preventDefault: () => void;
  }) => {
    if (streaming) return;
    const list = e.clipboardData?.files;
    if (list && list.length > 0) {
      e.preventDefault();
      await addFiles(list);
      return;
    }
    const items = e.clipboardData?.items;
    if (!items) return;
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
    }
  };

  const interruptBlocked = pendingInterrupts.length > 0;
  const canSend =
    !streaming &&
    !interruptBlocked &&
    (input.trim().length > 0 || attachments.length > 0);

  return (
    <section
      className="chat-pane"
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
                    <div className={`avatar ${m.error ? "error" : ""}`}>
                      {m.error ? "!" : "iC"}
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
                      {m.segments && m.segments.length > 0 ? (
                        <>
                          {m.segments.map((seg, segIdx) => {
                            if (seg.type === "reasoning") {
                              const isLast =
                                segIdx === m.segments!.length - 1;
                              const active = Boolean(
                                isStreamingBubble && isLast && !m.content,
                              );
                              return (
                                <MsgReasoning
                                  key={seg.id}
                                  reasoning={seg.text}
                                  active={active}
                                  durationSec={
                                    seg.durationSec ?? m.reasoningDurationSec
                                  }
                                  startedAtMs={
                                    active ? seg.at : undefined
                                  }
                                />
                              );
                            }
                            if (seg.type === "activity") {
                              const act = m.activities?.find(
                                (a) => a.id === seg.id,
                              );
                              if (!act) return null;
                              return (
                                <ActivityCards
                                  key={seg.id}
                                  items={[act]}
                                  prefs={displayPrefs}
                                  showTimestamps={displayPrefs.showTimestamps}
                                />
                              );
                            }
                            const surface = m.uiSurfaces?.find(
                              (s) => s.messageId === seg.id,
                            );
                            if (!surface) return null;
                            return (
                              <A2UIRenderer
                                key={seg.id}
                                operations={surface.operations}
                                disabled={surface.status !== "active"}
                                onAction={(name, context) =>
                                  onUiAction?.(m.id, name, context)
                                }
                              />
                            );
                          })}
                        </>
                      ) : (
                        <>
                          {m.reasoning ? (
                            <MsgReasoning
                              reasoning={m.reasoning}
                              active={reasoningActive}
                              durationSec={m.reasoningDurationSec}
                            />
                          ) : null}
                          {m.activities && m.activities.length > 0 && (
                            <ActivityCards
                              items={m.activities}
                              prefs={displayPrefs}
                              showTimestamps={displayPrefs.showTimestamps}
                            />
                          )}
                          {m.uiSurfaces && m.uiSurfaces.length > 0
                            ? m.uiSurfaces.map((surface) => (
                                <A2UIRenderer
                                  key={surface.messageId}
                                  operations={surface.operations}
                                  disabled={surface.status !== "active"}
                                  onAction={(name, context) =>
                                    onUiAction?.(m.id, name, context)
                                  }
                                />
                              ))
                            : null}
                        </>
                      )}
                      {displayPrefs.showTimestamps && m.createdAt ? (
                        <div className="msg-timestamp">
                          {new Date(m.createdAt).toLocaleTimeString()}
                        </div>
                      ) : null}
                      {isStreamingBubble &&
                      !m.content &&
                      !m.reasoning &&
                      !m.attachments?.length &&
                      !m.uiSurfaces?.length &&
                      !(
                        m.activities?.length &&
                        displayPrefs.verbosity !== "compact"
                      ) ? (
                        <MsgStreamLoader alone />
                      ) : (
                        <>
                          {m.content ? (
                            <ChatMarkdown
                              content={m.content}
                              streaming={isStreamingBubble}
                              compact={displayPrefs.verbosity === "compact"}
                              plain={m.role === "user" || Boolean(m.error)}
                              caret={isStreamingBubble}
                            />
                          ) : null}
                          {isStreamingBubble ? <MsgStreamLoader /> : null}
                        </>
                      )}
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
          if (canSend) onSend();
        }}
      >
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

        <div className="composer composer--stacked">
          <input
            ref={fileInputRef}
            type="file"
            className="composer-file-input"
            multiple
            accept="image/*,video/*,audio/*,.pdf,.txt,.md,.json,.csv,.doc,.docx,.xls,.xlsx,.ppt,.pptx,.zip,.rs,.ts,.tsx,.js,.py"
            onChange={(e) => void onFileChange(e)}
            disabled={streaming || attachments.length >= MAX_ATTACHMENTS}
          />
          <textarea
            ref={textareaRef}
            className="composer-input"
            value={input}
            rows={2}
            onChange={(e) => {
              const v = e.target.value;
              onInputChange(v);
              syncTriggerFromCaret(v, e.target.selectionStart ?? v.length);
            }}
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
            placeholder={
              streaming
                ? t("chat.placeholderStreaming")
                : interruptBlocked
                  ? t("chat.interrupt.pending")
                  : emptyMode === "chat"
                    ? t("chat.welcomePlaceholder")
                    : attachments.length
                      ? t("chat.placeholderWithAttach")
                      : t("chat.placeholder")
            }
            disabled={streaming || interruptBlocked}
            autoFocus
          />
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
                {modeMenuOpen ? (
                  <div className="composer-mode-menu" role="listbox">
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
                          {selected ? <Check size={14} strokeWidth={2.4} /> : null}
                        </button>
                      );
                    })}
                  </div>
                ) : null}
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

              <div className="composer-mcp-wrap">
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
                    setMcpOpen((v) => !v);
                  }}
                >
                  <McpIcon size={16} />
                </button>
                <ComposerMcpMenu
                  open={mcpOpen}
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
              <button
                type="button"
                className="composer-icon-btn composer-context-btn"
                disabled={streaming}
                title={
                  contextUsagePercent != null
                    ? `${t("chat.contextUsage")} · ${contextUsagePercent}%`
                    : t("chat.contextUsageHint")
                }
                aria-label={t("chat.contextUsage")}
                onClick={onOpenContext}
              >
                <ChartPie size={17} strokeWidth={2} />
                {contextUsagePercent != null ? (
                  <span className="composer-context-pct">{contextUsagePercent}%</span>
                ) : null}
              </button>
              <button
                type="button"
                className="composer-icon-btn"
                onClick={() => fileInputRef.current?.click()}
                disabled={streaming || attachments.length >= MAX_ATTACHMENTS}
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
  );
}
