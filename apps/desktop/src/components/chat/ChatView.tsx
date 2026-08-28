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
  Bot,
  Check,
  ChevronDown,
  CornerDownRight,
  ArrowDown,
  ArrowUp,
  ExternalLink,
  File,
  FileVideo,
  GitBranch,
  Hand,
  Image,
  Infinity as InfinityIcon,
  ListX,
  ListTree,
  Music2,
  Pencil,
  Plus,
  RefreshCw,
  SendHorizontal,
  ShieldAlert,
  ShieldCheck,
  Sparkles,
  Square,
  Trash2,
  X,
  MoreHorizontal,
} from "lucide-react";
import {
  Check as CheckData,
  ChevronDown as ChevronDownData,
  ChevronUp as ChevronUpData,
  Copy as CopyData,
  Pause as PauseData,
  Play as PlayData,
} from "lucide";
import { MorphToggleIcon } from "../icons/MorphIcon";
import {
  isActivityVisible,
  type ChatDisplayPrefs,
} from "../../hooks/chat/useChatDisplayPrefs";
import { useI18n } from "../../i18n/LocaleContext";
import {
  findSlotAt,
  listTemplateSegments,
  nextEmptySlot,
  prepareAgentCreateSend,
  prevEmptySlot,
} from "../../lib/agent/agentCreateTemplate";
import {
  CHAT_MODES,
  type ChatWorkMode,
  type ModeSwitchRequest,
} from "../../lib/chat/chatMode";
import type { QueuedFollowUp } from "../../lib/chat/followUpQueue";
import type { ParallelChatTask } from "../../lib/chat/parallelTasks";
import { countRunningParallel, countSettledByStatus, isParallelTaskActive } from "../../lib/chat/parallelTasks";
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
  ChatHistoryDto,
  ChatMessage,
  InstalledSkill,
  MessageTokenUsage,
  ModelCapabilities,
  ModelPricingMeta,
  ModelReasoningMeta,
  PendingInterrupt,
} from "../../types";
import {
  attachmentAcceptForCaps,
  attachmentKindAllowed,
  estimateTurnCostUsd,
  formatEstimateCostUsd,
} from "../../lib/model/modelCaps";
import { useTransientToast } from "../../hooks/ui/useTransientToast";
import { useConfirm } from "../../hooks/ui/DialogContext";
import AgentAvatar from "../agents/AgentAvatar";
import ChatMessageNav from "./ChatMessageNav";
import { ChatMarkdown } from "./ChatMarkdown";
import { ChatWelcome } from "./ChatWelcome";
import {
  ComposerPalette,
  type PaletteItem,
  type PaletteKind,
} from "./ComposerPalette";
import ComposerPlusMenu from "./ComposerPlusMenu";
import ComposerContextPreview, {
  type ComposerPreviewTarget,
} from "./ComposerContextPreview";
import ContextUsagePopover from "./ContextUsagePopover";
import McpIcon from "../icons/McpIcon";
import { ModelBrandIcon } from "../icons/ProviderIcons";
import MsgActivity from "./MsgActivity";
import MsgActivityGroup from "./MsgActivityGroup";
import MsgCitations from "./MsgCitations";
import MsgReasoning from "./MsgReasoning";
import MsgStreamLoader from "./MsgStreamLoader";
import { MsgTimeline, MsgTimelineStep, type MsgTimelineKind } from "./MsgTimeline";
import { useMcpTools } from "../../hooks/providers/useMcpTools";
import { useTypingPlaceholder } from "../../hooks/chat/useTypingPlaceholder";
import LocationA2UISurface from "./LocationA2UISurface";
import A2UISurfaceCard from "./A2UISurfaceCard";
import ComposerClarifySurface from "./ComposerClarifySurface";
import TodoProgress from "./TodoProgress";
import BrowserPreviewFloat from "./BrowserPreviewFloat";
import type { BrowserPreview } from "../../hooks/chat/useBrowserPreview";
import {
  CronRunFloatingCard,
  CronTaskDetailDrawer,
  CronRunDetailDrawer,
  cronRunStatusKind,
  type CronJobDto,
  type CronRunDto,
} from "../schedule/CronRunDetailDrawer";
import {
  CreateCronDialog,
  type ProviderOpt,
} from "../schedule/CreateCronDialog";
import { formatElapsedSec } from "../../lib/chat/elapsedSec";
import { mapHistoryMessages } from "../../lib/chat/mapHistoryMessages";
import { coalesceReasoningSegments } from "../../lib/chat/chatTimeline";
import {
  groupConsecutiveActivities,
  isConsecutiveActivityGroup,
} from "../../lib/chat/groupActivities";
import { findLastUserMessageId } from "../../lib/chat/messageEditing";
import { isLocationRequiredSurface } from "../../lib/chat/locationSurface";
import {
  findComposerClarifySurface,
  isClarifySurface,
} from "../../lib/chat/composerClarify";
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
import {
  addComposerContextToken,
  removeTriggerText,
  serializeComposerContext,
  type ComposerContextToken,
} from "../../lib/chat/composerContext";

const CHECK_ICON = CheckData;
const COPY_ICON = CopyData;
const PAUSE_ICON = PauseData;
const PLAY_ICON = PlayData;

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
  /** 当前父会话 id，用于展示其 Agent Threads。 */
  sessionId?: string | null;
  /** 当前会话消息（含欢迎占位） */
  messages: ChatMessage[];
  /** 项目文件打开时替换消息滚动区；输入框与任务条仍保留。 */
  workspaceContent?: ReactNode;
  /** 输入框文本 */
  input: string;
  /** 待发送附件 */
  attachments: ChatAttachment[];
  /** 是否正在流式生成 */
  streaming: boolean;
  /** 主会话整轮未结束（含 HITL）；用于软边界入队 */
  turnInFlight?: boolean;
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
  /** Agent/Plan 流式中的 follow-up 队列 */
  queuedFollowUps?: QueuedFollowUp[];
  onRemoveQueuedFollowUp?: (id: string) => void;
  onUpdateQueuedFollowUpText?: (id: string, text: string) => void;
  onMoveQueuedFollowUp?: (id: string, dir: -1 | 1) => void;
  onSteerQueuedFollowUp?: (id: string) => void | Promise<boolean>;
  onOpenQueuedFollowUpInNewTask?: (id: string) => void | Promise<boolean>;
  onCloseQueuedFollowUps?: () => boolean;
  /** `switch_mode` 流结束后的授权请求 */
  modeSwitchPrompt?: ModeSwitchRequest | null;
  onApproveModeSwitch?: () => void;
  onDismissModeSwitch?: () => void;
  /** 用户显式创建的独立任务 */
  parallelTasks?: ParallelChatTask[];
  onCancelParallelTask?: (id: string) => void;
  onWriteParallelSummary?: () => void;
  onClearSettledParallel?: () => void;
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
  /** 点击欢迎页示例 Prompt */
  onPickWelcomePrompt: (prompt: string) => void;
  /** DeepSeek 等支持 thinking 时显示输入框控件 */
  showThinkingControls?: boolean;
  /** OpenRouter 推理档位元数据；用于过滤思考级别 */
  reasoningMeta?: ModelReasoningMeta | null;
  thinkingPrefs: ChatThinkingPrefs;
  onToggleThinking: () => void;
  onThinkingLevelChange: (level: ThinkingLevel) => void;
  /** MCP 菜单作用域 Agent；缺省走 workspace */
  agentId?: string | null;
  /** 当前聊天模型 id：助手无自定义头像时用作品牌图标 */
  modelId?: string | null;
  /** 当前任务绑定浏览器的悬浮预览。 */
  browserPreview?: BrowserPreview | null;
  onCloseBrowserPreview?: () => void;
  /** 定时任务编辑器使用的供应商列表。 */
  cronProviders?: ProviderOpt[];
  cronActiveProviderId?: string | null;
  /** 当前模型输入能力（附件门禁） */
  modelCapabilities?: ModelCapabilities | null;
  /** 当前模型单价（估费预览） */
  modelPricing?: ModelPricingMeta | null;
  /** 打开 Tools 面板 MCP tab */
  onOpenMcpSettings?: () => void;
  /** Agent / Plan 工作模式 */
  chatMode: ChatWorkMode;
  onChatModeChange: (mode: ChatWorkMode) => void;
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
  /** 原位修改最后一条用户消息，并截断旧回答后重新执行 */
  onEditUserMessage?: (messageId: string, content: string) => Promise<boolean>;
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

type PermissionPreset = "ask_for_approval" | "approve_for_me" | "full_access";
type PermissionSettings = {
  preset: string | null;
  sandboxHealth: {
    backend: string;
    status: "available" | "unavailable";
    detail: string;
  };
};
const PERMISSION_PRESETS: PermissionPreset[] = [
  "ask_for_approval",
  "approve_for_me",
  "full_access",
];

function normalizePermissionPreset(
  value: string | null | undefined,
): PermissionPreset {
  return PERMISSION_PRESETS.includes(value as PermissionPreset)
    ? (value as PermissionPreset)
    : "ask_for_approval";
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
    kind === "image" || kind === "video" || kind === "audio"
      ? URL.createObjectURL(file)
      : undefined;

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

function ComposerContextGlyph({ kind }: { kind: ComposerContextToken["kind"] }) {
  if (kind === "skill") return <Sparkles size={15} strokeWidth={2} aria-hidden />;
  if (kind === "mcp") return <McpIcon size={15} />;
  return <Bot size={15} strokeWidth={2} aria-hidden />;
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

/** 消息悬停操作：AI 为复制/重新生成/分支，最后一条用户消息为复制/编辑。 */
export function MessageActions({
  messageId,
  content,
  role,
  disabled,
  onRegenerate,
  onEdit,
  onBranch,
}: {
  messageId: string;
  content: string;
  role: "user" | "assistant";
  disabled?: boolean;
  onRegenerate?: (messageId: string) => void;
  onEdit?: (messageId: string) => void;
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
            disabled={disabled || !onRegenerate}
            onClick={() => onRegenerate?.(messageId)}
            aria-label={t("chat.regenerate")}
            title={t("chat.regenerate")}
          >
            <RefreshCw size={14} strokeWidth={2} aria-hidden />
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
    !disabled && value.trim().length > 0 && value.trim() !== originalValue.trim();

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
    textarea.style.height = `${Math.min(textarea.scrollHeight, 240)}px`;
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

export default function ChatView({
  sessionId = null,
  messages,
  workspaceContent = null,
  input,
  attachments,
  streaming,
  turnInFlight = false,
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
  onSteerQueuedFollowUp,
  onOpenQueuedFollowUpInNewTask,
  onCloseQueuedFollowUps,
  modeSwitchPrompt = null,
  onApproveModeSwitch,
  onDismissModeSwitch,
  parallelTasks = [],
  onCancelParallelTask,
  onWriteParallelSummary,
  onClearSettledParallel,
  pendingInterrupts = [],
  onUiAction,
  onPauseStream,
  onResumeStream,
  onStopStream,
  onNewChat,
  onPickWelcomePrompt,
  agentId = null,
  modelId = null,
  browserPreview = null,
  onCloseBrowserPreview,
  cronProviders = [],
  cronActiveProviderId = null,
  modelCapabilities = null,
  modelPricing = null,
  onOpenMcpSettings,
  chatMode,
  onChatModeChange,
  onOpenContext,
  contextUsagePercent = null,
  contextUsage = null,
  contextWindow = 0,
  onRegenerateMessage,
  onEditUserMessage,
  onBranchMessage,
  onSlashAction,
}: Props) {
  const { t } = useI18n();
  const { showToast, toastHost } = useTransientToast();
  const confirm = useConfirm();
  const lastUserMessageId = useMemo(
    () => findLastUserMessageId(messages),
    [messages],
  );
  const [editingUserMessageId, setEditingUserMessageId] = useState<string | null>(null);
  const [editingUserDraft, setEditingUserDraft] = useState("");
  const [submittingUserEdit, setSubmittingUserEdit] = useState(false);
  const chatPaneRef = useRef<HTMLElement>(null);
  const bottomRef = useRef<HTMLDivElement>(null);
  const messageListRef = useRef<HTMLDivElement>(null);
  const composerShellRef = useRef<HTMLFormElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const typedHintRef = useRef<HTMLSpanElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const modeMenuRef = useRef<HTMLDivElement>(null);
  const modeMenuPanelRef = useRef<HTMLDivElement>(null);
  const queueMenuRef = useRef<HTMLDivElement>(null);
  const approvalRequestIdRef = useRef(0);
  const plusWrapRef = useRef<HTMLDivElement>(null);
  const contextWrapRef = useRef<HTMLDivElement>(null);
  const contextCloseTimerRef = useRef<number | null>(null);
  const [modeMenuOpen, setModeMenuOpen] = useState(false);
  const [approvalMode, setApprovalMode] =
    useState<PermissionPreset>("ask_for_approval");
  const [sandboxHealth, setSandboxHealth] =
    useState<PermissionSettings["sandboxHealth"] | null>(null);
  const [approvalBusy, setApprovalBusy] = useState(false);
  const [tasksOpen, setTasksOpen] = useState(true);
  const [editingQueueId, setEditingQueueId] = useState<string | null>(null);
  const [queueMenuId, setQueueMenuId] = useState<string | null>(null);
  const [contextPopoverOpen, setContextPopoverOpen] = useState(false);
  const [plusOpen, setPlusOpen] = useState(false);
  const [paletteKind, setPaletteKind] = useState<PaletteKind | null>(null);
  const [paletteQuery, setPaletteQuery] = useState("");
  const [paletteIndex, setPaletteIndex] = useState(0);
  const [triggerStart, setTriggerStart] = useState(0);
  const [agents, setAgents] = useState<AgentIconInfo[]>([]);
  const [activeAgentId, setActiveAgentId] = useState<string | null>(null);
  const [skills, setSkills] = useState<InstalledSkill[]>([]);
  const [composerContexts, setComposerContexts] = useState<
    ComposerContextToken[]
  >([]);
  const [previewTarget, setPreviewTarget] =
    useState<ComposerPreviewTarget | null>(null);
  const [mediaBaseDir, setMediaBaseDir] = useState<string | null>(null);
  const [cronRun, setCronRun] = useState<CronRunDto | null>(null);
  const [cronJob, setCronJob] = useState<CronJobDto | null>(null);
  const [cronJobRuns, setCronJobRuns] = useState<CronRunDto[]>([]);
  const [cronJobRunsLoading, setCronJobRunsLoading] = useState(false);
  const [cronTaskOpen, setCronTaskOpen] = useState(false);
  const [cronEditOpen, setCronEditOpen] = useState(false);
  const [cronBusy, setCronBusy] = useState(false);
  const [selectedCronRun, setSelectedCronRun] = useState<CronRunDto | null>(null);
  const [selectedCronMessages, setSelectedCronMessages] = useState<ChatMessage[]>([]);
  const [selectedCronTraceLoading, setSelectedCronTraceLoading] = useState(false);
  /** 创建 Agent：发送校验失败时高亮的必填槽 index */
  const [agentCreateMissing, setAgentCreateMissing] = useState<number[]>([]);
  const { servers: mcpServers } = useMcpTools(agentId);
  const mcpHasEnabled = mcpServers.some((s) => s.enabled);

  const addComposerContext = useCallback((token: ComposerContextToken) => {
    setComposerContexts((current) => addComposerContextToken(current, token));
    window.requestAnimationFrame(() => textareaRef.current?.focus());
  }, []);

  const removeComposerContext = useCallback((token: ComposerContextToken) => {
    setComposerContexts((current) =>
      current.filter(
        (item) => !(item.kind === token.kind && item.id === token.id),
      ),
    );
    setPreviewTarget((current) =>
      current?.type === "context" &&
      current.item.kind === token.kind &&
      current.item.id === token.id
        ? null
        : current,
    );
  }, []);

  useEffect(() => {
    setComposerContexts([]);
    setPreviewTarget(null);
  }, [sessionId]);

  const loadCronTask = useCallback(async (jobId: string) => {
    setCronJobRunsLoading(true);
    try {
      const [jobs, runs] = await Promise.all([
        invoke<CronJobDto[]>("list_cron_jobs"),
        invoke<CronRunDto[]>("list_cron_job_runs", { id: jobId }),
      ]);
      setCronJob(jobs.find((job) => job.id === jobId) ?? null);
      setCronJobRuns(runs);
    } catch (error) {
      setCronJob(null);
      setCronJobRuns([]);
      showToast(String(error));
    } finally {
      setCronJobRunsLoading(false);
    }
  }, [showToast]);

  useEffect(() => {
    let cancelled = false;
    let timer: number | null = null;
    setCronRun(null);
    setCronJob(null);
    setCronJobRuns([]);
    setCronTaskOpen(false);
    setCronEditOpen(false);
    setSelectedCronRun(null);
    setSelectedCronMessages([]);
    if (!sessionId) return;

    const refresh = async () => {
      try {
        const run = await invoke<CronRunDto | null>("get_cron_run_by_session", {
          sessionId,
        });
        if (cancelled) return;
        setCronRun(run);
        if (run) {
          setCronJobRuns((current) => {
            const next = current.filter((item) => item.id !== run.id);
            return [run, ...next];
          });
        }
        if (run && cronRunStatusKind(run.status) === "running") {
          timer = window.setTimeout(() => void refresh(), 1500);
        }
      } catch {
        if (!cancelled) setCronRun(null);
      }
    };

    void refresh();
    return () => {
      cancelled = true;
      if (timer != null) window.clearTimeout(timer);
    };
  }, [sessionId]);

  useEffect(() => {
    if (!cronRun?.job_id) return;
    void loadCronTask(cronRun.job_id);
  }, [cronRun?.job_id, loadCronTask]);

  useEffect(() => {
    if (!selectedCronRun) return;
    let cancelled = false;
    let timer: number | null = null;

    const refresh = async () => {
      try {
        const latest = await invoke<CronRunDto | null>("get_cron_run", {
          id: selectedCronRun.id,
        });
        if (cancelled) return;
        const resolved = latest ?? selectedCronRun;
        setSelectedCronRun(resolved);
        setCronJobRuns((current) =>
          current.map((run) => (run.id === resolved.id ? resolved : run)),
        );

        if (resolved.session_id && resolved.session_id !== sessionId) {
          setSelectedCronTraceLoading(true);
          const history = await invoke<ChatHistoryDto>("get_chat_history", {
            sessionId: resolved.session_id,
            limit: 200,
          });
          if (cancelled) return;
          setSelectedCronMessages(mapHistoryMessages(history.messages ?? []));
        }

        if (cronRunStatusKind(resolved.status) === "running") {
          timer = window.setTimeout(() => void refresh(), 1500);
        }
      } catch (error) {
        if (!cancelled) showToast(String(error));
      } finally {
        if (!cancelled) setSelectedCronTraceLoading(false);
      }
    };

    setSelectedCronMessages([]);
    void refresh();
    return () => {
      cancelled = true;
      if (timer != null) window.clearTimeout(timer);
    };
  }, [selectedCronRun?.id, sessionId, showToast]);

  const deleteSelectedCronRun = useCallback(async () => {
    if (!selectedCronRun) return;
    const approved = await confirm({
      title: t("dialog.deleteTitle"),
      message: t("cron.history.deleteRunConfirm"),
      confirmLabel: t("cron.history.deleteRun"),
      variant: "danger",
    });
    if (!approved) return;
    try {
      await invoke("delete_cron_run", { id: selectedCronRun.id });
      setCronJobRuns((current) =>
        current.filter((run) => run.id !== selectedCronRun.id),
      );
      setSelectedCronRun(null);
      setSelectedCronMessages([]);
    } catch (error) {
      showToast(String(error));
    }
  }, [confirm, selectedCronRun, showToast, t]);

  const toggleCronJob = useCallback(async () => {
    if (!cronJob || cronBusy) return;
    const previous = cronJob;
    const next = { ...cronJob, enabled: !cronJob.enabled };
    setCronJob(next);
    setCronBusy(true);
    try {
      const updated = await invoke<boolean>("set_cron_job_enabled", {
        id: cronJob.id,
        enabled: next.enabled,
      });
      if (!updated) throw new Error(t("cron.error"));
    } catch (error) {
      setCronJob(previous);
      showToast(String(error));
    } finally {
      setCronBusy(false);
    }
  }, [cronBusy, cronJob, showToast, t]);

  const runCronJobNow = useCallback(async () => {
    if (!cronJob || cronBusy) return;
    setCronBusy(true);
    try {
      const run = await invoke<CronRunDto>("run_cron_job_now", {
        id: cronJob.id,
      });
      setCronJobRuns((current) => [
        run,
        ...current.filter((item) => item.id !== run.id),
      ]);
      await loadCronTask(cronJob.id);
    } catch (error) {
      showToast(String(error));
    } finally {
      setCronBusy(false);
    }
  }, [cronBusy, cronJob, loadCronTask, showToast]);

  const cancelContextPopoverClose = useCallback(() => {
    if (contextCloseTimerRef.current == null) return;
    window.clearTimeout(contextCloseTimerRef.current);
    contextCloseTimerRef.current = null;
  }, []);

  const openContextPopover = useCallback(() => {
    cancelContextPopoverClose();
    setModeMenuOpen(false);
    setPlusOpen(false);
    setPaletteKind(null);
    setContextPopoverOpen(true);
  }, [cancelContextPopoverClose]);

  const closeContextPopover = useCallback(() => {
    cancelContextPopoverClose();
    setContextPopoverOpen(false);
  }, [cancelContextPopoverClose]);

  const scheduleContextPopoverClose = useCallback(() => {
    cancelContextPopoverClose();
    contextCloseTimerRef.current = window.setTimeout(() => {
      contextCloseTimerRef.current = null;
      setContextPopoverOpen(false);
    }, 160);
  }, [cancelContextPopoverClose]);

  useEffect(() => cancelContextPopoverClose, [cancelContextPopoverClose]);

  useEffect(() => {
    const pane = chatPaneRef.current;
    const composer = composerShellRef.current;
    if (!pane || !composer) return;

    const syncComposerOverlayHeight = () => {
      pane.style.setProperty(
        "--composer-overlay-height",
        `${Math.ceil(composer.getBoundingClientRect().height)}px`,
      );
    };

    syncComposerOverlayHeight();
    const observer =
      typeof ResizeObserver === "undefined"
        ? null
        : new ResizeObserver(syncComposerOverlayHeight);
    observer?.observe(composer);
    window.addEventListener("resize", syncComposerOverlayHeight);

    return () => {
      observer?.disconnect();
      window.removeEventListener("resize", syncComposerOverlayHeight);
      pane.style.removeProperty("--composer-overlay-height");
    };
  }, []);

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

  const refreshApprovalMode = useCallback(async () => {
    const requestId = ++approvalRequestIdRef.current;
    try {
      const settings = await invoke<PermissionSettings>("get_permission_settings");
      if (requestId === approvalRequestIdRef.current) {
        setApprovalMode(normalizePermissionPreset(settings.preset));
        setSandboxHealth(settings.sandboxHealth);
      }
    } catch {
      // 浏览器预览或后端暂不可用时保留安全默认值。
    }
  }, []);

  useEffect(() => {
    void refreshApprovalMode();
  }, [refreshApprovalMode]);

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
    attachments.length === 0 &&
    composerContexts.length === 0;
  useTypingPlaceholder(welcomeHints, typingPlaceholderEnabled, typedHintRef);

  useEffect(() => {
    if (!modeMenuOpen) return;
    void refreshApprovalMode();
    const onDoc = (ev: MouseEvent) => {
      const target = ev.target as Node;
      if (modeMenuRef.current?.contains(target)) return;
      if (modeMenuPanelRef.current?.contains(target)) return;
      setModeMenuOpen(false);
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [modeMenuOpen, refreshApprovalMode]);

  useEffect(() => {
    if (!queueMenuId) return;
    const onPointerDown = (event: MouseEvent) => {
      if (queueMenuRef.current?.contains(event.target as Node)) return;
      setQueueMenuId(null);
    };
    const onKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape") setQueueMenuId(null);
    };
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [queueMenuId]);

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
    sizeKey: `${chatMode}:${approvalMode}`,
  });

  const modeMeta = useMemo(() => {
    const map: Record<
      ChatWorkMode,
      { label: string; desc: string; Icon: typeof InfinityIcon }
    > = {
      agent: {
        label: t("chat.modeAgent"),
        desc: t("chat.mode.desc.agent"),
        Icon: InfinityIcon,
      },
      plan: {
        label: t("chat.modePlan"),
        desc: t("chat.mode.desc.plan"),
        Icon: ListTree,
      },
    };
    return map;
  }, [t]);

  const approvalMeta = useMemo(
    () => ({
      ask_for_approval: {
        label: t("chat.approval.askForApproval"),
        desc: t("chat.approval.desc.askForApproval"),
        Icon: Hand,
      },
      approve_for_me: {
        label: t("chat.approval.approveForMe"),
        desc: t("chat.approval.desc.approveForMe"),
        Icon: ShieldCheck,
      },
      full_access: {
        label: t("chat.approval.fullAccess"),
        desc: t("chat.approval.desc.fullAccess"),
        Icon: ShieldAlert,
      },
    }),
    [t],
  );

  const changeApprovalMode = useCallback(
    async (next: PermissionPreset) => {
      if (approvalBusy || next === approvalMode) {
        setModeMenuOpen(false);
        return;
      }
      setModeMenuOpen(false);
      let confirmed = false;
      if (next === "full_access") {
        const approved = await confirm({
          title: t("chat.approval.fullAccessConfirmTitle"),
          message: t("chat.approval.fullAccessConfirmMessage"),
          confirmLabel: t("chat.approval.enableFullAccess"),
          variant: "danger",
        });
        if (!approved) return;
        confirmed = true;
      }
      setApprovalBusy(true);
      const requestId = ++approvalRequestIdRef.current;
      try {
        const settings = await invoke<PermissionSettings>(
          "set_permission_preset",
          { preset: next, confirmed },
        );
        if (requestId === approvalRequestIdRef.current) {
          setApprovalMode(normalizePermissionPreset(settings.preset));
          setSandboxHealth(settings.sandboxHealth);
        }
      } catch (error) {
        showToast(t("chat.approval.updateFailed", { error: String(error) }), {
          tone: "error",
        });
      } finally {
        setApprovalBusy(false);
      }
    },
    [approvalBusy, approvalMode, confirm, showToast, t],
  );

  const slashItems: PaletteItem[] = useMemo(() => {
    return buildSlashPaletteEntries(skills).map((e) => {
      const skill =
        e.action === "insert_skill" && e.skillName
          ? skills.find((item) => item.name === e.skillName)
          : undefined;
      return {
        id: e.id,
        title: e.title,
        description: e.description ?? t(e.descKey),
        action: e.action,
        icon: e.icon,
        skillName: e.skillName,
        contextToken: skill
          ? {
              id: skill.id,
              kind: "skill" as const,
              name: skill.name,
              description: skill.description,
              path: skill.path,
            }
          : undefined,
      };
    });
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
        icon,
        mentionKind: c.kind,
        action: "insert" as const,
        contextToken: {
          id: c.id,
          kind: c.kind,
          name: c.name,
          description: c.description,
          path:
            c.kind === "skill"
              ? skills.find((skill) => skill.id === c.id)?.path
              : undefined,
        },
      };
    });
  }, [agents, skills, mcpServers, t]);

  const activePaletteItems = useMemo(() => {
    if (paletteKind === "slash") return slashItems;
    if (paletteKind === "mention") return mentionItems;
    return [];
  }, [paletteKind, slashItems, mentionItems]);

  const filteredPaletteItems = useMemo(() => {
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
        setComposerContexts([]);
        setPreviewTarget(null);
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
      if (item.contextToken) {
        const end = textareaRef.current?.selectionStart ?? input.length;
        onInputChange(removeTriggerText(input, triggerStart, end));
        addComposerContext(item.contextToken);
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
      closePalette,
      onInputChange,
      addComposerContext,
      runSlashAction,
      insertAtTrigger,
      triggerStart,
      input.length,
    ],
  );

  const syncTriggerFromCaret = useCallback(
    (text: string, caret: number) => {
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
      if (composerContexts.length === 0 && tryHandleSlashSubmit()) return;
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

  const fileAccept = useMemo(
    () => attachmentAcceptForCaps(modelCapabilities),
    [modelCapabilities],
  );

  const estimateCostLabel = useMemo(() => {
    const inChars =
      input.length +
      attachments.reduce((n, a) => n + (a.name?.length ?? 0) + 64, 0) +
      composerContexts.reduce((n, item) => n + item.name.length + 2, 0);
    const outTokens = 1024;
    const usd = estimateTurnCostUsd({
      pricing: modelPricing,
      inputChars: inChars,
      expectedOutputTokens: outTokens,
    });
    const cost = formatEstimateCostUsd(usd);
    if (!cost) return null;
    const inTok = Math.max(0, Math.ceil(inChars / 4));
    return {
      cost,
      short: t("chat.estimateCostShort", { cost }),
      full: t("chat.estimateCost", {
        cost,
        in: String(inTok),
        out: String(outTokens),
      }),
    };
  }, [attachments, composerContexts, input.length, modelPricing, t]);

  // 切换模型后丢掉不再支持的附件
  useEffect(() => {
    if (!modelCapabilities || attachments.length === 0) return;
    const kept: ChatAttachment[] = [];
    let dropped = 0;
    for (const a of attachments) {
      if (attachmentKindAllowed(a.kind, modelCapabilities)) kept.push(a);
      else {
        revokePreview(a);
        dropped += 1;
      }
    }
    if (dropped === 0) return;
    onAttachmentsChange(kept);
    showToast(t("chat.attachmentUnsupported", { n: String(dropped) }), {
      error: true,
    });
    // 仅在能力变化时清理；attachments 本身由本 effect 更新
    // eslint-disable-next-line react-hooks/exhaustive-deps -- intentional
  }, [modelCapabilities]);

  const addFiles = useCallback(
    async (files: FileList | File[]) => {
      const list = Array.from(files);
      if (!list.length) return;
      const room = MAX_ATTACHMENTS - attachments.length;
      if (room <= 0) return;
      const nextBatch = list.slice(0, room);
      const created = await Promise.all(nextBatch.map((f) => fileToAttachment(f)));
      const allowed = created.filter((a) =>
        attachmentKindAllowed(a.kind, modelCapabilities),
      );
      const skipped = created.length - allowed.length;
      for (const a of created) {
        if (!attachmentKindAllowed(a.kind, modelCapabilities)) revokePreview(a);
      }
      if (skipped > 0) {
        showToast(t("chat.attachmentUnsupported", { n: String(skipped) }), {
          error: true,
        });
      }
      if (!allowed.length) return;
      onAttachmentsChange([...attachments, ...allowed]);
    },
    [attachments, modelCapabilities, onAttachmentsChange, showToast, t],
  );

  const addAttachments = useCallback(
    (created: ChatAttachment[]) => {
      if (!created.length) return;
      const room = MAX_ATTACHMENTS - attachments.length;
      if (room <= 0) return;
      const sliced = created.slice(0, room);
      const allowed = sliced.filter((a) =>
        attachmentKindAllowed(a.kind, modelCapabilities),
      );
      const skipped = sliced.length - allowed.length;
      for (const a of sliced) {
        if (!attachmentKindAllowed(a.kind, modelCapabilities)) revokePreview(a);
      }
      if (skipped > 0) {
        showToast(t("chat.attachmentUnsupported", { n: String(skipped) }), {
          error: true,
        });
      }
      if (!allowed.length) return;
      onAttachmentsChange([...attachments, ...allowed]);
    },
    [attachments, modelCapabilities, onAttachmentsChange, showToast, t],
  );

  const addPaths = useCallback(
    async (paths: string[]) => {
      if (!paths.length) return;
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
    setPreviewTarget((current) =>
      current?.type === "attachment" && current.item.id === id ? null : current,
    );
    onAttachmentsChange(attachments.filter((a) => a.id !== id));
  };

  const onFileChange = async (e: ChangeEvent<HTMLInputElement>) => {
    if (e.target.files) await addFiles(e.target.files);
    e.target.value = "";
  };

  const onDragEnter = (e: DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
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
  const composerClarify = useMemo(
    () => findComposerClarifySurface(messages, pendingInterrupts),
    [messages, pendingInterrupts],
  );
  const canQueueWhileBusy =
    streaming || turnInFlight || interruptBlocked;
  const canSend =
    !sendBlocked &&
    (input.trim().length > 0 ||
      attachments.length > 0 ||
      composerContexts.length > 0) &&
    (!streaming || canQueueWhileBusy);
  const parallelRunningCount = useMemo(
    () => countRunningParallel(parallelTasks),
    [parallelTasks],
  );
  /** 主会话流式或同一 turn 未收束时显示 Stop；独立任务在各自卡片停止。 */
  const showStopControl = streaming || turnInFlight;
  const showPauseResume = streaming;
  const showSendButton = !streaming && !composerClarify;
  const modeSwitchLocked = streaming || turnInFlight || interruptBlocked;
  const contextProgress =
    typeof contextUsagePercent === "number" &&
    Number.isFinite(contextUsagePercent)
      ? Math.min(100, Math.max(0, contextUsagePercent))
      : null;
  const contextProgressTone =
    contextProgress == null
      ? "is-unknown"
      : contextProgress >= 90
        ? "is-critical"
        : contextProgress >= 70
          ? "is-warning"
          : "is-normal";
  const contextUsageLabel =
    contextProgress != null
      ? `${t("chat.contextUsage")} · ${contextProgress}%`
      : t("chat.contextUsageHint");
  const parallelRunningIds = useMemo(
    () =>
      new Set(
        parallelTasks
          .filter((task) => isParallelTaskActive(task.status))
          .map((task) => task.assistantMessageId),
      ),
    [parallelTasks],
  );

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
      onSend({ text: serializeComposerContext(composerContexts, prep.sanitized) });
      onInputChange("");
      setComposerContexts([]);
      setPreviewTarget(null);
      return;
    }
    if (composerContexts.length > 0) {
      onSend({ text: serializeComposerContext(composerContexts, input) });
      onInputChange("");
    } else {
      onSend();
    }
    setComposerContexts([]);
    setPreviewTarget(null);
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

  const composerPlaceholder =
    streaming || turnInFlight
      ? t("chat.placeholderStreaming")
      : sendBlocked && sendBlockedReason
        ? sendBlockedReason
        : emptyMode === "chat"
            ? ""
            : attachments.length
              ? t("chat.placeholderWithAttach")
              : chatMode === "plan"
                ? t("chat.placeholderPlan")
                : t("chat.placeholder");

  useEffect(() => {
    if (
      editingUserMessageId &&
      (editingUserMessageId !== lastUserMessageId || !onEditUserMessage)
    ) {
      setEditingUserMessageId(null);
      setEditingUserDraft("");
      setSubmittingUserEdit(false);
    }
  }, [editingUserMessageId, lastUserMessageId, onEditUserMessage]);

  const beginUserMessageEdit = useCallback(
    (message: ChatMessage) => {
      if (
        message.id !== lastUserMessageId ||
        streaming ||
        turnInFlight ||
        sendBlocked ||
        !onEditUserMessage
      ) {
        return;
      }
      setEditingUserMessageId(message.id);
      setEditingUserDraft(message.content);
    },
    [lastUserMessageId, onEditUserMessage, sendBlocked, streaming, turnInFlight],
  );

  const cancelUserMessageEdit = useCallback(() => {
    if (submittingUserEdit) return;
    setEditingUserMessageId(null);
    setEditingUserDraft("");
  }, [submittingUserEdit]);

  const submitUserMessageEdit = useCallback(async () => {
    const messageId = editingUserMessageId;
    const content = editingUserDraft.trim();
    if (!messageId || !content || !onEditUserMessage || submittingUserEdit) return;
    setSubmittingUserEdit(true);
    try {
      const accepted = await onEditUserMessage(messageId, content);
      if (!accepted) return;
      setEditingUserMessageId(null);
      setEditingUserDraft("");
    } finally {
      setSubmittingUserEdit(false);
    }
  }, [editingUserDraft, editingUserMessageId, onEditUserMessage, submittingUserEdit]);

  return (
    <ChatMediaAttachProvider value={mediaAttachApi}>
    <section
      ref={chatPaneRef}
      className={`chat-pane ${workspaceContent ? "has-project-file" : ""} ${
        fileDragOver ? "is-file-dragover" : ""
      } ${cronRun && !workspaceContent ? "has-cron-run" : ""}`.trim()}
      onDragEnter={onDragEnter}
      onDragLeave={onDragLeave}
      onDragOver={(e) => {
        e.preventDefault();
        e.stopPropagation();
      }}
      onDrop={(e) => void onDrop(e)}
    >
      {toastHost}
      {cronRun && !workspaceContent ? (
        <aside className="chat-cron-run-float">
          <CronRunFloatingCard
            run={cronRun}
            onOpen={() => setCronTaskOpen(true)}
          />
        </aside>
      ) : null}
      {browserPreview && !workspaceContent ? (
        <BrowserPreviewFloat
          preview={browserPreview}
          onClose={() => onCloseBrowserPreview?.()}
        />
      ) : null}
      {workspaceContent ? (
        workspaceContent
      ) : emptyMode === "chat" || emptyMode === "agent" ? (
        <ChatWelcome
          onPickCard={onPickWelcomePrompt}
          onActivate={() => textareaRef.current?.focus()}
        />
      ) : (
        <div className="message-list-wrap">
          <div className="message-list" ref={messageListRef}>
            {messages.map((m, index) => {
              const isStreamingBubble =
                m.role === "assistant" &&
                !m.error &&
                (parallelRunningIds.has(m.id) ||
                  (streaming && index === messages.length - 1));
              const reasoningActive = Boolean(
                isStreamingBubble && m.reasoning && !m.content,
              );
              const isEditingUserMessage = editingUserMessageId === m.id;
              const canEditUserMessage =
                m.role === "user" &&
                m.id === lastUserMessageId &&
                !streaming &&
                !turnInFlight &&
                !sendBlocked &&
                Boolean(onEditUserMessage);
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
                      } ${isStreamingBubble && (m.content || m.reasoning) ? "is-streaming" : ""}${
                        isEditingUserMessage ? " is-editing" : ""
                      }`}
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
                      ) : (() => {
                        type Step = {
                          key: string;
                          kind: MsgTimelineKind;
                          active?: boolean;
                          activity?: ChatActivity;
                          node: ReactNode;
                        };
                        const steps: Step[] = [];
                        const pushActivity = (act: ChatActivity) => {
                          if (!isActivityVisible(act.kind, displayPrefs)) return;
                          steps.push({
                            key: `act-${act.id}`,
                            kind: act.kind,
                            active: act.status === "running",
                            activity: act,
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
                          if (m.citations?.length) {
                            steps.push({
                              key: `cite-${m.id}`,
                              kind: "reasoning" as MsgTimelineKind,
                              active: false,
                              node: <MsgCitations citations={m.citations} />,
                            });
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
                                <MsgStreamLoader visible={isStreamingBubble} />
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

                        const groupedSteps = groupConsecutiveActivities(steps);

                        if (!hasProcess) {
                          return <>{steps.map((step) => (
                            <div key={step.key}>{step.node}</div>
                          ))}</>;
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
                                    active={activities.some(
                                      (activity) => activity.status === "running",
                                    )}
                                    isLast={isLast}
                                  >
                                    <MsgActivityGroup
                                      activities={activities}
                                      showTimestamp={displayPrefs.showTimestamps}
                                      mediaBaseDir={mediaBaseDir}
                                    />
                                  </MsgTimelineStep>
                                );
                              }
                              return (
                                <MsgTimelineStep
                                  key={step.key}
                                  kind={step.kind}
                                  active={step.active}
                                  isLast={isLast}
                                >
                                  {step.node}
                                </MsgTimelineStep>
                              );
                            })}
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
                    m.id !== "welcome" &&
                    (m.role === "assistant" ||
                      (canEditUserMessage && !isEditingUserMessage)) ? (
                      <MessageActions
                        messageId={m.id}
                        content={m.content}
                        role={m.role}
                        disabled={streaming}
                        onRegenerate={
                          m.role === "assistant" ? onRegenerateMessage : undefined
                        }
                        onEdit={
                          m.role === "user"
                            ? () => beginUserMessageEdit(m)
                            : undefined
                        }
                        onBranch={
                          m.role === "assistant" ? onBranchMessage : undefined
                        }
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

      <TodoProgress messages={messages} />

      <form
        ref={composerShellRef}
        className="composer-shell"
        onSubmit={(e) => {
          e.preventDefault();
          if (composerContexts.length === 0 && tryHandleSlashSubmit()) return;
          trySubmitComposer();
        }}
      >
        {modeSwitchPrompt?.to === "agent" && (
          <div
            className="composer-queue composer-mode-switch"
            role="status"
            aria-live="polite"
          >
            <div className="composer-mode-switch-body">
              <span className="composer-mode-switch-title">
                {t("chat.modeSwitch.title")}
              </span>
              <span className="composer-mode-switch-reason">
                {modeSwitchPrompt.reason}
              </span>
              {modeSwitchPrompt.summary ? (
                <span className="composer-mode-switch-summary">
                  {modeSwitchPrompt.summary}
                </span>
              ) : null}
            </div>
            <span className="composer-queue-actions">
              <button
                type="button"
                className="composer-queue-btn"
                onClick={() => onApproveModeSwitch?.()}
              >
                {t("chat.modeSwitch.execute")}
              </button>
              <button
                type="button"
                className="composer-queue-btn"
                onClick={() => onDismissModeSwitch?.()}
              >
                {t("chat.modeSwitch.keepPlanning")}
              </button>
            </span>
          </div>
        )}
        {queuedFollowUps.length > 0 && (
          <div
            className={`composer-queue ${queueMenuId ? "has-open-menu" : ""}`.trim()}
            aria-label={t("chat.queue.title", { count: String(queuedFollowUps.length) })}
          >
              {queuedFollowUps.map((item, index) => {
                const isSteering = item.delivery === "steering";
                return (
                  <div
                    key={item.id}
                    className={`composer-queue-card ${isSteering ? "is-steering" : ""}`.trim()}
                  >
                    <span className="composer-queue-card-icon" aria-hidden>
                      <CornerDownRight size={14} strokeWidth={2} />
                    </span>
                    {editingQueueId === item.id && !isSteering ? (
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
                      <span className="composer-queue-card-text">
                        {item.text.trim() || t("chat.queue.emptyText")}
                        {item.attachments.length > 0
                          ? ` · ${item.attachments.length} 📎`
                          : ""}
                      </span>
                    )}
                    <div className="composer-queue-card-actions">
                      <button
                        type="button"
                        className="composer-queue-steer"
                        title={t("chat.queue.steer")}
                        aria-label={t("chat.queue.steer")}
                        disabled={!sessionId || !turnInFlight || isSteering}
                        onClick={() => {
                          setQueueMenuId(null);
                          void onSteerQueuedFollowUp?.(item.id);
                        }}
                      >
                        <CornerDownRight size={13} strokeWidth={2.1} />
                        <span>
                          {isSteering ? t("chat.queue.steering") : t("chat.queue.steer")}
                        </span>
                      </button>
                      <button
                        type="button"
                        className="composer-queue-btn"
                        title={t("chat.queue.remove")}
                        aria-label={t("chat.queue.remove")}
                        disabled={isSteering}
                        onClick={() => onRemoveQueuedFollowUp?.(item.id)}
                      >
                        <Trash2 size={13} strokeWidth={2} />
                      </button>
                      <button
                        type="button"
                        className="composer-queue-btn composer-queue-menu-btn"
                        title={t("chat.queue.more")}
                        aria-label={t("chat.queue.more")}
                        aria-haspopup="menu"
                        aria-expanded={queueMenuId === item.id}
                        disabled={isSteering}
                        onClick={() => setQueueMenuId((current) => current === item.id ? null : item.id)}
                      >
                        <MoreHorizontal size={14} strokeWidth={2} />
                      </button>
                      {queueMenuId === item.id && !isSteering ? (
                        <div
                          ref={queueMenuRef}
                          className="composer-queue-menu"
                          role="menu"
                          aria-label={t("chat.queue.more")}
                        >
                          <button
                            type="button"
                            role="menuitem"
                            onClick={() => {
                              setQueueMenuId(null);
                              setEditingQueueId(item.id);
                            }}
                          >
                            <Pencil size={14} strokeWidth={2} />
                            <span>{t("chat.queue.edit")}</span>
                          </button>
                          <button
                            type="button"
                            role="menuitem"
                            onClick={() => {
                              setQueueMenuId(null);
                              void onOpenQueuedFollowUpInNewTask?.(item.id);
                            }}
                          >
                            <ExternalLink size={14} strokeWidth={2} />
                            <span>{t("chat.queue.openInNewTask")}</span>
                          </button>
                          <button
                            type="button"
                            role="menuitem"
                            disabled={index === 0}
                            onClick={() => {
                              setQueueMenuId(null);
                              onMoveQueuedFollowUp?.(item.id, -1);
                            }}
                          >
                            <ArrowUp size={14} strokeWidth={2} />
                            <span>{t("chat.queue.moveUp")}</span>
                          </button>
                          <button
                            type="button"
                            role="menuitem"
                            disabled={index === queuedFollowUps.length - 1}
                            onClick={() => {
                              setQueueMenuId(null);
                              onMoveQueuedFollowUp?.(item.id, 1);
                            }}
                          >
                            <ArrowDown size={14} strokeWidth={2} />
                            <span>{t("chat.queue.moveDown")}</span>
                          </button>
                          <span className="composer-queue-menu-separator" role="separator" />
                          <button
                            type="button"
                            role="menuitem"
                            onClick={() => {
                              if (onCloseQueuedFollowUps?.()) setQueueMenuId(null);
                            }}
                          >
                            <ListX size={14} strokeWidth={2} />
                            <span>{t("chat.queue.close")}</span>
                          </button>
                        </div>
                      ) : null}
                    </div>
                  </div>
                );
              })}
          </div>
        )}

        {parallelTasks.length > 0 && (
          <div
            className="composer-queue composer-tasks"
            aria-label={t("chat.task.title", {
              running: String(parallelRunningCount),
              total: String(parallelTasks.length),
            })}
          >
            <button
              type="button"
              className="composer-queue-toggle"
              aria-expanded={tasksOpen}
              onClick={() => setTasksOpen((o) => !o)}
            >
              <MorphToggleIcon
                active={tasksOpen}
                activeIcon={ChevronUpData}
                inactiveIcon={ChevronDownData}
                size={14}
                strokeWidth={2.2}
                aria-hidden
              />
              <span>
                {t("chat.task.title", {
                  running: String(parallelRunningCount),
                  total: String(parallelTasks.length),
                })}
              </span>
            </button>
            {tasksOpen && (
              <ul className="composer-queue-list">
                {parallelTasks.map((task) => (
                  <li
                    key={task.id}
                    className={`composer-queue-item is-task is-${task.status}`}
                  >
                    <span
                      className={`composer-queue-dot is-${task.status}`}
                      aria-hidden
                    />
                    <span className="composer-queue-text">
                      <span className="composer-task-status">
                        {task.status === "running"
                          ? t("chat.task.status.running")
                          : task.status === "waiting"
                            ? t("chat.task.status.waiting")
                            : task.status === "done"
                              ? t("chat.task.status.done")
                              : task.status === "error"
                                ? t("chat.task.status.error")
                                : t("chat.task.status.cancelled")}
                      </span>
                      {task.prompt.trim() || t("chat.queue.emptyText")}
                      {task.worktree?.path ? (
                        <span className="composer-task-worktree" title={task.worktree.path}>
                          {task.worktree.path.split(/[/\\]/).slice(-2).join("/")}
                        </span>
                      ) : null}
                    </span>
                    {isParallelTaskActive(task.status) && (
                      <span className="composer-queue-actions">
                        <button
                          type="button"
                          className="composer-queue-btn"
                          title={t("chat.task.cancel")}
                          aria-label={t("chat.task.cancel")}
                          onClick={() => onCancelParallelTask?.(task.id)}
                        >
                          <Square size={12} strokeWidth={2.4} />
                        </button>
                      </span>
                    )}
                  </li>
                ))}
              </ul>
            )}
            {countRunningParallel(parallelTasks) === 0 &&
              parallelTasks.length > 0 && (
                <div className="composer-task-summary">
                  {(() => {
                    const s = countSettledByStatus(parallelTasks);
                    return (
                      <span className="composer-task-summary-text">
                        {t("chat.task.summaryCounts", {
                          done: String(s.done),
                          error: String(s.error),
                          cancelled: String(s.cancelled),
                        })}
                      </span>
                    );
                  })()}
                  <span className="composer-queue-actions">
                    <button
                      type="button"
                      className="composer-queue-btn"
                      onClick={() => onWriteParallelSummary?.()}
                    >
                      {t("chat.task.writeSummary")}
                    </button>
                    <button
                      type="button"
                      className="composer-queue-btn"
                      onClick={() => onClearSettledParallel?.()}
                    >
                      {t("chat.task.clearSettled")}
                    </button>
                  </span>
                </div>
              )}
          </div>
        )}

        {paletteKind ? (
          <ComposerPalette
            kind={paletteKind}
            items={activePaletteItems}
            query={paletteQuery}
            activeIndex={paletteIndex}
            onHover={setPaletteIndex}
            onSelect={applyPaletteItem}
            onClose={closePalette}
          />
        ) : null}

        <div
          className={`composer composer--stacked ${composerClarify ? "has-clarify" : ""} ${fileDragOver ? "is-file-dragover" : ""}`.trim()}
        >
          <input
            ref={fileInputRef}
            type="file"
            className="composer-file-input"
            multiple
            accept={fileAccept || undefined}
            onChange={(e) => void onFileChange(e)}
            disabled={
              attachments.length >= MAX_ATTACHMENTS || fileAccept === ""
            }
          />
          {fileDragOver ? (
            <div className="composer-drop-hint" aria-live="polite">
              {t("chat.dropFilesHint")}
            </div>
          ) : null}
          {attachments.length > 0 || composerContexts.length > 0 ? (
            <div
              className="composer-previews composer-context-strip"
              aria-label={t("chat.contextStripLabel")}
            >
              {attachments.map((att) => (
                <div key={att.id} className="composer-preview" data-kind={att.kind}>
                  <button
                    type="button"
                    className="composer-preview-open"
                    onClick={() =>
                      setPreviewTarget({ type: "attachment", item: att })
                    }
                    aria-label={t("chat.previewContext", { name: att.name })}
                    title={t("chat.previewContext", { name: att.name })}
                  >
                    {att.kind === "image" && att.previewUrl ? (
                      <img src={att.previewUrl} alt="" />
                    ) : att.kind === "video" && att.previewUrl ? (
                      <video src={att.previewUrl} muted />
                    ) : (
                      <span className="composer-preview-icon" data-kind={att.kind}>
                        <AttachmentGlyph kind={att.kind} />
                      </span>
                    )}
                    <span className="composer-preview-meta">
                      <span className="composer-preview-name">{att.name}</span>
                      <span className="composer-preview-size">
                        {formatSize(att.size)}
                      </span>
                    </span>
                  </button>
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
              {composerContexts.map((token) => {
                const kindLabel = {
                  agent: t("chat.contextToken.agent"),
                  skill: t("chat.contextToken.skill"),
                  mcp: t("chat.contextToken.mcp"),
                }[token.kind];
                return (
                  <div
                    key={`${token.kind}:${token.id}`}
                    className="composer-context-token"
                    data-kind={token.kind}
                  >
                    <button
                      type="button"
                      className="composer-context-token-open"
                      onClick={() =>
                        setPreviewTarget({ type: "context", item: token })
                      }
                      aria-label={t("chat.previewContext", { name: token.name })}
                      title={t("chat.previewContext", { name: token.name })}
                    >
                      <span className="composer-context-token-icon">
                        <ComposerContextGlyph kind={token.kind} />
                      </span>
                      <span className="composer-context-token-meta">
                        <span className="composer-context-token-kind">
                          {kindLabel}
                        </span>
                        <span className="composer-context-token-name">
                          {token.name}
                        </span>
                      </span>
                    </button>
                    <button
                      type="button"
                      className="composer-preview-remove"
                      onClick={() => removeComposerContext(token)}
                      aria-label={t("chat.removeContext", { name: token.name })}
                      title={t("chat.removeContext", { name: token.name })}
                      disabled={streaming}
                    >
                      ×
                    </button>
                  </div>
                );
              })}
            </div>
          ) : null}
          {!composerClarify && emptyMode === "agent" && agentCreateMissing.length > 0 ? (
            <p className="composer-agent-validate-hint" role="alert">
              {t("chat.agentCreateNeedRequired")}
            </p>
          ) : null}
          {composerClarify ? (
            <ComposerClarifySurface
              surface={composerClarify.surface}
              mediaBaseDir={mediaBaseDir}
              onAction={(name, context) =>
                onUiAction?.(composerClarify.messageId, name, context)
              }
            />
          ) : (
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
              disabled={sendBlocked}
              autoFocus
            />
          </div>
          )}
          <div className="composer-bar">
            <div className="composer-bar-left">
              <div className="composer-mode" ref={modeMenuRef}>
                <button
                  type="button"
                  className={`composer-mode-pill composer-policy-pill ${modeMenuOpen ? "is-open" : ""} ${approvalMode === "full_access" ? "is-full-access" : ""}`.trim()}
                  disabled={approvalBusy}
                  aria-haspopup="menu"
                  aria-expanded={modeMenuOpen}
                  aria-label={`${t("chat.modeMenu")} · ${t("chat.approval.menu")}`}
                  title={`${modeMeta[chatMode].label} · ${approvalMeta[approvalMode].label}`}
                  onClick={() => {
                    setPlusOpen(false);
                    setContextPopoverOpen(false);
                    setPaletteKind(null);
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
                        <span className="composer-policy-separator" aria-hidden>·</span>
                        <span className="composer-policy-approval">
                          {approvalMeta[approvalMode].label}
                        </span>
                        <ChevronDown size={14} strokeWidth={2} />
                      </>
                    );
                  })()}
                </button>
                {modeMenuOpen && typeof document !== "undefined"
                  ? createPortal(
                      <div
                        ref={modeMenuPanelRef}
                        className="composer-mode-menu composer-policy-menu"
                        role="menu"
                        style={modeMenuStyle ?? { visibility: "hidden" }}
                      >
                        <div className="composer-policy-section-label">
                          {t("chat.modeMenu")}
                        </div>
                        {CHAT_MODES.map((mode) => {
                          const Meta = modeMeta[mode];
                          const Icon = Meta.Icon;
                          const selected = mode === chatMode;
                          return (
                            <button
                              key={mode}
                              type="button"
                              role="menuitemradio"
                              aria-checked={selected}
                              disabled={modeSwitchLocked}
                              className={`composer-mode-item ${selected ? "is-selected" : ""}`}
                              onClick={() => {
                                onChatModeChange(mode);
                                setModeMenuOpen(false);
                              }}
                            >
                              <Icon size={16} strokeWidth={2} />
                              <span className="composer-mode-item-text">
                                <span className="composer-mode-item-label">{Meta.label}</span>
                                <span className="composer-mode-item-desc">{Meta.desc}</span>
                              </span>
                              {selected ? (
                                <Check size={14} strokeWidth={2.4} />
                              ) : null}
                            </button>
                          );
                        })}
                        <div className="composer-policy-divider" />
                        <div className="composer-policy-section-label">
                          {t("chat.approval.menu")}
                        </div>
                        {PERMISSION_PRESETS.map((mode) => {
                          const Meta = approvalMeta[mode];
                          const Icon = Meta.Icon;
                          const selected = mode === approvalMode;
                          return (
                            <button
                              key={mode}
                              type="button"
                              role="menuitemradio"
                              aria-checked={selected}
                              data-approval-mode={mode}
                              className={`composer-mode-item composer-approval-item ${selected ? "is-selected" : ""}`}
                              onClick={() => void changeApprovalMode(mode)}
                            >
                              <Icon size={16} strokeWidth={2} />
                              <span className="composer-mode-item-text">
                                <span className="composer-mode-item-label">
                                  {Meta.label}
                                  {mode === "ask_for_approval" ? (
                                    <span className="composer-approval-recommended">
                                      {t("chat.approval.recommended")}
                                    </span>
                                  ) : null}
                                </span>
                                <span className="composer-mode-item-desc">{Meta.desc}</span>
                              </span>
                              {selected ? <Check size={14} strokeWidth={2.4} /> : null}
                            </button>
                          );
                        })}
                        <div
                          className="composer-approval-health"
                          data-status={sandboxHealth?.status ?? "unknown"}
                        >
                          <span className="composer-approval-health-dot" aria-hidden />
                          <span>
                            {sandboxHealth?.status === "available"
                              ? t("chat.approval.sandboxAvailable")
                              : t("chat.approval.sandboxUnavailable")}
                          </span>
                        </div>
                      </div>,
                      document.body,
                    )
                  : null}
              </div>

              <div className="composer-mcp-wrap" ref={plusWrapRef}>
                <button
                  type="button"
                  className={`composer-icon-btn composer-plus-btn ${plusOpen ? "is-open" : ""} ${
                    mcpHasEnabled ? "has-dot" : ""
                  }`}
                  disabled={streaming}
                  title={t("chat.plusMenu")}
                  aria-label={t("chat.plusMenu")}
                  aria-haspopup="dialog"
                  aria-expanded={plusOpen}
                  onClick={() => {
                    setModeMenuOpen(false);
                    setPaletteKind(null);
                    setContextPopoverOpen(false);
                    setPlusOpen((value) => !value);
                  }}
                >
                  <Plus size={17} strokeWidth={2.2} />
                </button>
                <ComposerPlusMenu
                  open={plusOpen}
                  anchorRef={plusWrapRef}
                  agentId={agentId}
                  skills={skills}
                  canAttach={
                    attachments.length < MAX_ATTACHMENTS && fileAccept !== ""
                  }
                  onAttach={() => fileInputRef.current?.click()}
                  onSelectSkill={(skill) =>
                    addComposerContext({
                      id: skill.id,
                      kind: "skill",
                      name: skill.name,
                      description: skill.description,
                      path: skill.path,
                    })
                  }
                  onSelectMcp={(server) =>
                    addComposerContext({
                      id: server.id,
                      kind: "mcp",
                      name: server.name,
                      description: server.description,
                    })
                  }
                  onClose={() => setPlusOpen(false)}
                  onOpenSettings={() => onOpenMcpSettings?.()}
                />
              </div>

            </div>

            <div className="composer-bar-right">
              <div
                className="composer-context-wrap"
                ref={contextWrapRef}
                onPointerEnter={openContextPopover}
                onPointerLeave={scheduleContextPopoverClose}
              >
                <button
                  type="button"
                  className={`composer-icon-btn composer-context-btn ${contextProgressTone} ${
                    contextPopoverOpen ? "is-open" : ""
                  }`}
                  disabled={streaming}
                  title={contextUsageLabel}
                  aria-label={contextUsageLabel}
                  aria-expanded={contextPopoverOpen}
                  onFocus={openContextPopover}
                  onClick={openContextPopover}
                >
                  <svg
                    className="composer-context-ring"
                    viewBox="0 0 24 24"
                    aria-hidden
                  >
                    <circle
                      className="composer-context-ring-track"
                      cx="12"
                      cy="12"
                      r="9"
                      pathLength="100"
                    />
                    {contextProgress != null ? (
                      <circle
                        className="composer-context-ring-value"
                        cx="12"
                        cy="12"
                        r="9"
                        pathLength="100"
                        strokeDasharray={`${contextProgress} 100`}
                      />
                    ) : null}
                  </svg>
                </button>
                <ContextUsagePopover
                  open={contextPopoverOpen}
                  snapshot={contextUsage}
                  windowTokens={contextWindow}
                  estimateCost={estimateCostLabel}
                  containRef={contextWrapRef}
                  onPointerEnter={cancelContextPopoverClose}
                  onPointerLeave={scheduleContextPopoverClose}
                  onClose={closeContextPopover}
                  onViewDetails={() => {
                    onOpenContext();
                  }}
                />
              </div>
              {showStopControl ? (
                <>
                  {showPauseResume ? (
                    <button
                      type="button"
                      className="composer-icon-btn"
                      onClick={() =>
                        streamPaused ? onResumeStream?.() : onPauseStream?.()
                      }
                      title={t(streamPaused ? "chat.streamResume" : "chat.streamPause")}
                      aria-label={t(streamPaused ? "chat.streamResume" : "chat.streamPause")}
                    >
                      <MorphToggleIcon
                        active={streamPaused}
                        activeIcon={PLAY_ICON}
                        inactiveIcon={PAUSE_ICON}
                        size={17}
                        strokeWidth={2.2}
                      />
                    </button>
                  ) : null}
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
              ) : null}
              {showSendButton ? (
                <button
                  className="send-btn send-btn--round"
                  type="submit"
                  disabled={!canSend}
                  aria-label={t("chat.send")}
                  title={t("chat.send")}
                >
                  <SendHorizontal size={17} strokeWidth={2.2} />
                </button>
              ) : null}
            </div>
          </div>
        </div>
      </form>
        <ComposerContextPreview
          target={previewTarget}
          onClose={() => setPreviewTarget(null)}
        />
      </section>
      {cronJob && cronTaskOpen ? (
        <CronTaskDetailDrawer
          job={cronJob}
          runs={cronJobRuns}
          runsLoading={cronJobRunsLoading}
          busy={cronBusy}
          nonModal
          onClose={() => setCronTaskOpen(false)}
          onEdit={() => setCronEditOpen(true)}
          onToggleEnabled={() => void toggleCronJob()}
          onRunNow={() => void runCronJobNow()}
          onOpenRun={setSelectedCronRun}
        />
      ) : null}
      {selectedCronRun ? (
        <CronRunDetailDrawer
          run={selectedCronRun}
          messages={
            selectedCronRun.session_id === sessionId
              ? messages
              : selectedCronMessages
          }
          traceLoading={selectedCronTraceLoading}
          onClose={() => setSelectedCronRun(null)}
          onDelete={() => void deleteSelectedCronRun()}
        />
      ) : null}
      <CreateCronDialog
        open={Boolean(cronJob && cronEditOpen)}
        editingJob={cronJob}
        onClose={() => setCronEditOpen(false)}
        onCreated={() => {
          if (cronJob) void loadCronTask(cronJob.id);
        }}
        providers={cronProviders}
        activeProviderId={cronActiveProviderId}
      />
    </ChatMediaAttachProvider>
  );
}
