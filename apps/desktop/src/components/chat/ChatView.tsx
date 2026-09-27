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
import { motion, useReducedMotion } from "framer-motion";
import { useClampPopover } from "../../hooks/ui/useClampPopover";
import {
  Bot,
  Check,
  ChevronDown,
  CircleStop,
  CornerDownRight,
  ArrowDown,
  ArrowUp,
  ExternalLink,
  File,
  Hand,
  Infinity as InfinityIcon,
  ListEnd,
  ListX,
  ListTree,
  Mic,
  Navigation,
  Pencil,
  Plus,
  ShieldAlert,
  ShieldCheck,
  Sparkles,
  Square,
  PhoneOff,
  Trash2,
  MoreHorizontal,
  Maximize2,
  Minimize2,
} from "lucide-react";
import {
  ChevronDown as ChevronDownData,
  ChevronUp as ChevronUpData,
  Pause as PauseData,
  Play as PlayData,
} from "lucide";
import { MorphToggleIcon } from "../icons/MorphIcon";
import {
  type ChatAnswerLayout,
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
  CHAT_SEND_MODES,
  type ChatSendMode,
  type ChatWorkMode,
  type ModeSwitchRequest,
  loadChatSendMode,
  saveChatSendMode,
} from "../../lib/chat/chatMode";
import {
  findPromptTemplateSlotAt,
  listPromptTemplateSegments,
  nextEmptyPromptTemplateSlot,
  preparePromptTemplateSend,
  prevEmptyPromptTemplateSlot,
} from "../../lib/chat/promptTemplate";
import type { QueuedFollowUp } from "../../lib/chat/followUpQueue";
import type { ParallelChatTask } from "../../lib/chat/parallelTasks";
import {
  countRunningParallel,
  countSettledByStatus,
  isParallelTaskActive,
} from "../../lib/chat/parallelTasks";
import { pendingAsyncQuestionsAt } from "../../lib/chat/asyncAgentUpdate";
import type { ContextUsageSnapshot } from "../../lib/chat/contextUsage";
import { ChatMediaAttachProvider } from "../../contexts/ChatMediaAttachContext";
import TaskCompletionCelebration from "./TaskCompletionCelebration";
import {
  attachmentsFromOsClipboard,
  filesFromClipboardRead,
  pathToAttachment,
  pathsFromClipboardText,
  pathsFromDataTransfer,
  pathsToAttachments,
} from "../../lib/chat/chatPaste";
import type { MediaActionKind } from "../../lib/media/mediaActions";
import type {
  ChatThinkingPrefs,
  ThinkingLevel,
} from "../../lib/chat/thinkingPrefs";
import {
  assistantAnswerPlainText,
  assistantProcessMarkdown,
} from "../../lib/chat/assistantTurnClipboard";
import type {
  ChatAttachment,
  ChatAttachmentKind,
  ChatEmptyMode,
  ResponseItemHistoryDto,
  ConversationEntry,
  InstalledSkill,
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
import TurnNavigator from "./TurnNavigator";
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
import ChatMessageRow from "./ChatMessageRow";
import { AttachmentGlyph, formatSize } from "./attachmentDisplay";
import AssistantTurnContextMenu, {
  type AssistantTurnMenuAction,
} from "./AssistantTurnContextMenu";
import { useMcpTools } from "../../hooks/providers/useMcpTools";
import { useTypingPlaceholder } from "../../hooks/chat/useTypingPlaceholder";
import { useRealtimeConversation } from "../../hooks/chat/useRealtimeConversation";
import ComposerClarifySurface from "./ComposerClarifySurface";
import ConversationInteractions from "../desktop-pet/ConversationInteractions";
import { usePendingInteractions } from "../../hooks/chat/usePendingInteractions";
import { inlineInteraction } from "../../lib/chat/pendingInteractions";
import TodoProgress from "./TodoProgress";
import { type FileChangeItem } from "../../lib/chat/taskProgress";
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
import { projectResponseItemsToEntries } from "../../lib/chat/projectResponseItemsToEntries";
import { findLastUserEntryId } from "../../lib/chat/turnEditing";
import { findComposerClarifySurface } from "../../lib/chat/composerClarify";
import { isAgentIconSrc, type AgentIconInfo } from "../../lib/agent/agentIcons";
import {
  buildMentionCandidates,
  buildSlashPaletteEntries,
  parseSlashInput,
  type SlashAction,
} from "../../lib/chat/composerCommands";
import {
  addComposerContextToken,
  createFileComposerContextToken,
  removeTriggerText,
  serializeComposerContext,
  type ComposerContextToken,
} from "../../lib/chat/composerContext";

const PAUSE_ICON = PauseData;
const PLAY_ICON = PlayData;

/** 格式化 token/s 展示（整数不带小数） */

/** 单条消息底部的易读摘要与可展开用量详情。 */

/** ChatView 入参：消息列表、输入态与流式控制回调 */
type Props = {
  projectId?: string | null;
  /** 当前父会话 id，用于展示其 Agent Threads。 */
  sessionId?: string | null;
  /** 当前会话消息（含欢迎占位） */
  messages: ConversationEntry[];
  /** 项目文件打开时替换消息滚动区；输入框与任务条仍保留。 */
  workspaceContent?: ReactNode;
  /** 内容专注态下将输入框收束为底部悬浮胶囊。 */
  composerPresentation?: "default" | "capsule";
  /** 同步悬浮输入区高度，用于为原生 WebView 预留可交互空间。 */
  onComposerHeightChange?: (height: number) => void;
  /** 同步胶囊上方的浮层状态，避免原生 WebView 遮挡菜单。 */
  onComposerOverlayOpenChange?: (open: boolean) => void;
  /** 输入框文本 */
  input: string;
  /** 从其它页面带入输入框的结构化上下文标签。 */
  composerContextPrefill?: ComposerContextToken | null;
  /** 上下文标签被输入框接收后清空父级暂存。 */
  onComposerContextPrefillConsumed?: () => void;
  /** 待发送附件 */
  attachments: ChatAttachment[];
  /** 是否正在流式生成 */
  streaming: boolean;
  /** 主会话整轮未结束（含 HITL）；用于软边界入队 */
  turnInFlight?: boolean;
  /** 成功任务完成序号；递增时播放内容区庆祝动画。 */
  completionCelebrationId?: number;
  /** 流是否已暂停 */
  streamPaused?: boolean;
  /** 禁止发送（压实中 / 只读会话等） */
  sendBlocked?: boolean;
  /** Missing model blocks submission, but keeps the draft editable. */
  modelUnavailable?: boolean;
  /** 禁止发送时的输入框占位/原因文案 */
  sendBlockedReason?: string;
  /** 聊天展示偏好（详细度等） */
  displayPrefs: ChatDisplayPrefs;
  /** 将单条回答的布局提升为全局默认。 */
  onDefaultAnswerLayoutChange?: (layout: ChatAnswerLayout) => void;
  /** 空状态模式：欢迎 / 创建 Agent / 正常 */
  emptyMode: ChatEmptyMode;
  /** 需要滚入视口的消息 id（用后应调用 onFocusConsumed） */
  focusMessageId?: string | null;
  /** 焦点滚动完成后由父组件清空 focusMessageId */
  onFocusConsumed?: () => void;
  onInputChange: (v: string) => void;
  onAttachmentsChange: (next: ChatAttachment[]) => void;
  /** 发送当前输入 */
  onSend: (opts?: { text?: string; sendMode?: ChatSendMode }) => void;
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
  /** Realtime 凭据解析所需的 provider 条目与 backend id。 */
  realtimeProviderId?: string | null;
  realtimeBackendId?: string | null;
  realtimeAvailable?: boolean;
  /** 当前任务绑定浏览器的悬浮预览。 */
  browserPreview?: BrowserPreview | null;
  onCloseBrowserPreview?: () => void;
  /** 在 Astro 内置浏览器中打开工具活动的 Web 目标。 */
  onOpenActivityUrl?: (url: string) => void | Promise<void>;
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
  /** 点击任务进度中的文件改动，打开工作区审查面板。 */
  onOpenFileReview?: (file: FileChangeItem, files: FileChangeItem[]) => void;
  /** 简易上下文占用 0–100，用于按钮提示 */
  contextUsagePercent?: number | null;
  /** 本轮上下文分层占用快照；无则浮层空态 */
  contextUsage?: ContextUsageSnapshot | null;
  /** 模型上下文窗口（tokens），默认 128K */
  contextWindow?: number;
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
const COMPOSER_LAYOUT_TRANSITION = {
  type: "spring",
  bounce: 0,
  duration: 0.4,
} as const;
const COMPOSER_REDUCED_MOTION_TRANSITION = { duration: 0 } as const;

type PermissionPreset = "ask_for_approval" | "approve_for_me" | "full_access";

/** 本会话额外权限（`request_permissions` 批准后由运行时写入）。 */
type SessionPermissionGrantsDto = {
  workspaceWrite: boolean;
  writableRoots: string[];
};
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
  if (
    ["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "heic"].includes(ext)
  ) {
    return "image";
  }
  if (["mp4", "webm", "mov", "mkv", "avi"].includes(ext)) return "video";
  if (["mp3", "wav", "m4a", "aac", "ogg", "flac"].includes(ext)) return "audio";
  return "file";
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

function ComposerContextGlyph({
  kind,
}: {
  kind: ComposerContextToken["kind"];
}) {
  if (kind === "skill")
    return <Sparkles size={15} strokeWidth={2} aria-hidden />;
  if (kind === "mcp") return <McpIcon size={15} />;
  if (kind === "file") return <File size={15} strokeWidth={2} aria-hidden />;
  return <Bot size={15} strokeWidth={2} aria-hidden />;
}

/** 消息内附件缩略图条 */

/** 消息悬停操作：AI 为复制/分支，最后一条用户消息为复制/编辑。 */

export default function ChatView({
  projectId = null,
  sessionId = null,
  messages,
  workspaceContent = null,
  composerPresentation = "default",
  onComposerHeightChange,
  onComposerOverlayOpenChange,
  input,
  composerContextPrefill = null,
  onComposerContextPrefillConsumed,
  attachments,
  streaming,
  turnInFlight = false,
  completionCelebrationId = 0,
  streamPaused = false,
  sendBlocked = false,
  modelUnavailable = false,
  sendBlockedReason,
  displayPrefs,
  onDefaultAnswerLayoutChange,
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
  realtimeProviderId = null,
  realtimeBackendId = null,
  realtimeAvailable = false,
  browserPreview = null,
  onCloseBrowserPreview,
  onOpenActivityUrl,
  cronProviders = [],
  cronActiveProviderId = null,
  modelCapabilities = null,
  modelPricing = null,
  onOpenMcpSettings,
  chatMode,
  onChatModeChange,
  onOpenContext,
  onOpenFileReview,
  contextUsagePercent = null,
  contextUsage = null,
  contextWindow = 0,
  onEditUserMessage,
  onBranchMessage,
  onSlashAction,
}: Props) {
  const { t } = useI18n();
  const reduceComposerMotion = useReducedMotion();
  const { showToast, toastHost } = useTransientToast();
  const realtime = useRealtimeConversation({
    sessionId,
    providerId: realtimeProviderId,
    backendId: realtimeBackendId,
  });
  const realtimeErrorRef = useRef<string | null>(null);
  useEffect(() => {
    if (!realtime.error || realtime.error === realtimeErrorRef.current) return;
    realtimeErrorRef.current = realtime.error;
    showToast(realtime.error, { error: true });
  }, [realtime.error, showToast]);
  const confirm = useConfirm();
  const lastUserMessageId = useMemo(
    () => findLastUserEntryId(messages),
    [messages],
  );
  const [editingUserMessageId, setEditingUserMessageId] = useState<
    string | null
  >(null);
  const [editingUserDraft, setEditingUserDraft] = useState("");
  const [submittingUserEdit, setSubmittingUserEdit] = useState(false);
  const [assistantMenu, setAssistantMenu] = useState<{
    messageId: string;
    x: number;
    y: number;
  } | null>(null);
  const assistantMenuReturnFocusRef = useRef<HTMLButtonElement | null>(null);
  const [messageLayoutOverrides, setMessageLayoutOverrides] = useState<
    Record<string, ChatAnswerLayout>
  >({});
  const [messageProcessExpanded, setMessageProcessExpanded] = useState<
    Record<string, boolean>
  >({});
  const chatPaneRef = useRef<HTMLElement>(null);
  const bottomRef = useRef<HTMLDivElement>(null);
  const messageListRef = useRef<HTMLDivElement>(null);
  const followLatestRef = useRef(true);
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
  const sendModeWrapRef = useRef<HTMLDivElement>(null);
  const sendModePanelRef = useRef<HTMLDivElement>(null);
  const [sendMode, setSendMode] = useState<ChatSendMode>(loadChatSendMode);
  const [sendModeOpen, setSendModeOpen] = useState(false);
  const [isScrolledFromBottom, setIsScrolledFromBottom] = useState(false);
  const [approvalMode, setApprovalMode] =
    useState<PermissionPreset>("ask_for_approval");
  const [sandboxHealth, setSandboxHealth] = useState<
    PermissionSettings["sandboxHealth"] | null
  >(null);
  const [approvalBusy, setApprovalBusy] = useState(false);
  const [sessionGrants, setSessionGrants] =
    useState<SessionPermissionGrantsDto | null>(null);
  const [tasksOpen, setTasksOpen] = useState(true);
  const [editingQueueId, setEditingQueueId] = useState<string | null>(null);
  const [queueMenuId, setQueueMenuId] = useState<string | null>(null);
  const [contextPopoverOpen, setContextPopoverOpen] = useState(false);
  const [plusOpen, setPlusOpen] = useState(false);
  const [composerManuallyExpanded, setComposerManuallyExpanded] =
    useState(false);
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
  const [selectedCronRun, setSelectedCronRun] = useState<CronRunDto | null>(
    null,
  );
  const [selectedCronMessages, setSelectedCronMessages] = useState<
    ConversationEntry[]
  >([]);
  const [selectedCronTraceLoading, setSelectedCronTraceLoading] =
    useState(false);
  /** 创建 Agent：发送校验失败时高亮的必填槽 index */
  const [agentCreateMissing, setAgentCreateMissing] = useState<number[]>([]);
  /** 欢迎页示例：当前模板的未填占位文案。 */
  const [welcomeTemplateHints, setWelcomeTemplateHints] = useState<
    string[] | null
  >(null);
  const [welcomeTemplateMissing, setWelcomeTemplateMissing] = useState<
    number[]
  >([]);
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
    setAssistantMenu(null);
    assistantMenuReturnFocusRef.current = null;
    setMessageLayoutOverrides({});
    setMessageProcessExpanded({});
  }, [sessionId]);

  useEffect(() => {
    if (!composerContextPrefill) return;
    setComposerContexts((current) =>
      addComposerContextToken(current, composerContextPrefill),
    );
    onComposerContextPrefillConsumed?.();
    window.requestAnimationFrame(() => textareaRef.current?.focus());
  }, [composerContextPrefill, onComposerContextPrefillConsumed]);

  const loadCronTask = useCallback(
    async (jobId: string) => {
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
    },
    [showToast],
  );

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
          const history = await invoke<ResponseItemHistoryDto>(
            "get_chat_history",
            {
              sessionId: resolved.session_id,
              limit: 200,
            },
          );
          if (cancelled) return;
          setSelectedCronMessages(
            projectResponseItemsToEntries(history.items ?? []),
          );
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

  const toggleCronJobArchived = useCallback(async () => {
    if (!cronJob || cronBusy) return;
    setCronBusy(true);
    try {
      const updated = await invoke<CronJobDto>("archive_cron_job", {
        id: cronJob.id,
        archived: !cronJob.archived_at,
      });
      setCronJob(updated);
    } catch (error) {
      showToast(String(error));
    } finally {
      setCronBusy(false);
    }
  }, [cronBusy, cronJob, showToast]);

  const cancelContextPopoverClose = useCallback(() => {
    if (contextCloseTimerRef.current == null) return;
    window.clearTimeout(contextCloseTimerRef.current);
    contextCloseTimerRef.current = null;
  }, []);

  const openContextPopover = useCallback(() => {
    cancelContextPopoverClose();
    setModeMenuOpen(false);
    setSendModeOpen(false);
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
      const height = Math.ceil(composer.getBoundingClientRect().height);
      pane.style.setProperty("--composer-overlay-height", `${height}px`);
      onComposerHeightChange?.(height);
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
  }, [onComposerHeightChange]);

  const loadMentionSources = useCallback(async () => {
    // 媒体预览只依赖本地工作区路径，不应被较重的 Agent 配置加载失败连带清空。
    // 该命令不依赖 backend 会话，应用刚启动时也能先让 generated/... 可解析。
    try {
      const workspace = await invoke<string>("get_default_workspace_path");
      const normalized = workspace.trim();
      if (normalized) setMediaBaseDir(normalized);
    } catch {
      // 保留已有路径；后续 get_config 成功时仍会刷新。
    }
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
      const configuredWorkspace =
        scoped?.path?.trim() || cfg.workspace_dir?.trim();
      if (configuredWorkspace) setMediaBaseDir(configuredWorkspace);
    } catch {
      setAgents([]);
      setActiveAgentId(null);
      // Agent 列表失败不影响已经解析出的媒体工作区。
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
      const settings = await invoke<PermissionSettings>(
        "get_permission_settings",
      );
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
    if (!sendModeOpen) return;
    const onDoc = (ev: MouseEvent) => {
      const target = ev.target as Node;
      if (sendModeWrapRef.current?.contains(target)) return;
      if (sendModePanelRef.current?.contains(target)) return;
      setSendModeOpen(false);
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [sendModeOpen]);

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

  const sendModeStyle = useClampPopover({
    open: sendModeOpen,
    anchorRef: sendModeWrapRef,
    popoverRef: sendModePanelRef,
    mode: "fixed",
    preferAlign: "end",
    placement: "above",
    gap: 8,
    maxHeightCap: 280,
    minMaxHeight: 96,
    sizeKey: sendMode,
  });

  const sendModeMeta = useMemo(
    () => ({
      queue: {
        label: t("chat.sendMode.queue"),
        desc: t("chat.sendMode.queue.desc"),
        Icon: ListEnd,
      },
      steer: {
        label: t("chat.sendMode.steer"),
        desc: t("chat.sendMode.steer.desc"),
        Icon: Navigation,
      },
      interrupt: {
        label: t("chat.sendMode.interrupt"),
        desc: t("chat.sendMode.interrupt.desc"),
        Icon: CircleStop,
      },
    }),
    [t],
  );

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

  // 打开权限菜单时读一次本会话额外权限；撤销走同一条 RPC。
  useEffect(() => {
    if (!modeMenuOpen || !sessionId) return;
    let cancelled = false;
    void invoke<SessionPermissionGrantsDto>("get_session_permission_grants", {
      sessionId,
    })
      .then((grants) => {
        if (!cancelled) setSessionGrants(grants);
      })
      .catch(() => {
        if (!cancelled) setSessionGrants(null);
      });
    return () => {
      cancelled = true;
    };
  }, [modeMenuOpen, sessionId]);

  const revokeSessionGrants = useCallback(async () => {
    if (!sessionId) return;
    try {
      const grants = await invoke<SessionPermissionGrantsDto>(
        "revoke_session_permission_grants",
        { sessionId },
      );
      setSessionGrants(grants);
      showToast?.(t("chat.approval.sessionGrantsRevoked"), {
        tone: "success",
      });
    } catch (error) {
      showToast?.(
        error instanceof Error ? error.message : String(error),
        { tone: "error" },
      );
    }
  }, [sessionId, showToast, t]);

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
    const candidates = buildMentionCandidates(
      {
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
      },
      t,
    );
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
        group: c.group,
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
        if (paletteKind === "slash" || paletteKind === "mention")
          closePalette();
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
          (i) =>
            (i - 1 + filteredPaletteItems.length) % filteredPaletteItems.length,
        );
        return;
      }
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        applyPaletteItem(
          filteredPaletteItems[paletteIndex] ?? filteredPaletteItems[0],
        );
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        closePalette();
        return;
      }
      if (e.key === "Tab") {
        e.preventDefault();
        applyPaletteItem(
          filteredPaletteItems[paletteIndex] ?? filteredPaletteItems[0],
        );
        return;
      }
    }

    const welcomeTemplateActive =
      emptyMode === "chat" && Boolean(welcomeTemplateHints?.length);
    if (e.key === "Tab" && (emptyMode === "agent" || welcomeTemplateActive)) {
      const el = textareaRef.current;
      if (!el) return;
      const caret = el.selectionStart ?? 0;
      const slot = welcomeTemplateActive
        ? e.shiftKey
          ? prevEmptyPromptTemplateSlot(
              input,
              caret,
              welcomeTemplateHints ?? [],
            )
          : nextEmptyPromptTemplateSlot(
              input,
              caret,
              welcomeTemplateHints ?? [],
            )
        : e.shiftKey
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

  const updateConversationScrollState = useCallback(() => {
    const list = messageListRef.current;
    if (!list) return;
    const distanceFromBottom =
      list.scrollHeight - list.scrollTop - list.clientHeight;
    const awayFromBottom = distanceFromBottom > 56;
    followLatestRef.current = !awayFromBottom;
    setIsScrolledFromBottom((current) =>
      current === awayFromBottom ? current : awayFromBottom,
    );
  }, []);

  const scrollConversationToBottom = useCallback(() => {
    followLatestRef.current = true;
    const list = messageListRef.current;
    list?.scrollTo({ top: list.scrollHeight, behavior: "smooth" });
  }, []);

  useEffect(() => {
    followLatestRef.current = true;
    setIsScrolledFromBottom(false);
  }, [emptyMode, sessionId]);

  useEffect(() => {
    if (focusMessageId) return;
    if (!followLatestRef.current) return;
    const list = messageListRef.current;
    list?.scrollTo({
      top: list.scrollHeight,
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
      timer = window.setTimeout(
        () => el.classList.remove("is-focus-flash"),
        1600,
      );
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
    const nextHeight = composerManuallyExpanded
      ? Math.min(Math.max(el.scrollHeight, 220), 360)
      : Math.min(el.scrollHeight, 120);
    el.style.height = `${nextHeight}px`;
  }, [composerManuallyExpanded, input]);

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
      const created = await Promise.all(
        nextBatch.map((f) => fileToAttachment(f)),
      );
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

  const addFolder = useCallback(async () => {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        directory: true,
        multiple: false,
        title: t("chat.plusMenuFolder"),
      });
      if (!selected || typeof selected !== "string") return;
      if (
        attachments.some(
          (attachment) =>
            attachment.kind === "folder" && attachment.localPath === selected,
        )
      ) {
        return;
      }
      const name = selected.split(/[/\\]/).filter(Boolean).pop() || selected;
      addAttachments([
        {
          id: `folder-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
          name,
          mime: "inode/directory",
          kind: "folder",
          size: 0,
          localPath: selected,
        },
      ]);
    } catch (error) {
      showToast(error instanceof Error ? error.message : String(error), {
        error: true,
      });
    }
  }, [addAttachments, attachments, showToast, t]);

  const addPaths = useCallback(
    async (paths: string[]) => {
      if (!paths.length) return;
      const created = await pathsToAttachments(paths);
      addAttachments(created);
    },
    [addAttachments, streaming, chatMode],
  );

  const attachMediaPath = useCallback(
    async (path: string, kind: MediaActionKind) => {
      if (kind === "code" || kind === "html" || kind === "document") {
        const description =
          kind === "code"
            ? t("media.kind.code")
            : kind === "html"
              ? t("media.kind.html")
              : t("chat.contextToken.file");
        addComposerContext(createFileComposerContextToken(path, description));
        if (!input.trim()) {
          onInputChange(
            t(
              kind === "code"
                ? "media.quoteCodePrompt"
                : "media.quoteFilePrompt",
            ),
          );
        }
        window.requestAnimationFrame(() => {
          textareaRef.current?.focus();
        });
        return;
      }
      const att = await pathToAttachment(path);
      addAttachments([att]);
      if (!input.trim()) {
        onInputChange(t("media.quotePrompt"));
      }
      window.requestAnimationFrame(() => {
        textareaRef.current?.focus();
      });
    },
    [addAttachments, addComposerContext, input, onInputChange, t],
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
    const clipboardTypes = Array.from(e.clipboardData?.types ?? []);
    const insertClipboardText = () => {
      const el = textareaRef.current;
      if (!el || !text) return;
      const start = el.selectionStart ?? el.value.length;
      const end = el.selectionEnd ?? start;
      const next = el.value.slice(0, start) + text + el.value.slice(end);
      onInputChange(next);
      const caret = start + text.length;
      window.requestAnimationFrame(() => {
        el.setSelectionRange(caret, caret);
      });
    };
    const pathList = pathsFromClipboardText(text);
    if (pathList.length) {
      e.preventDefault();
      const created = await pathsToAttachments(pathList);
      if (created.length) {
        addAttachments(created);
        return;
      }
      insertClipboardText();
      return;
    }

    // 普通文字交给 textarea 原生粘贴；不要让较慢的系统文件剪贴板探测阻塞输入。
    if (text || clipboardTypes.includes("text/html")) return;

    // 系统文件剪贴板（复制媒体后）在 WKWebView 里常不进 clipboardData.files
    // 仅在事件本身没有文字/文件时拦截，再读 OS 剪贴板或图片像素。
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
  };

  const interruptBlocked = pendingInterrupts.length > 0;
  const pendingInteractionState = usePendingInteractions();
  const hasLiveInteraction = pendingInteractionState.snapshot.requests.some(
    (r) => r.sessionId === sessionId && inlineInteraction(r),
  );
  const composerClarify = useMemo(
    () => findComposerClarifySurface(messages, pendingInterrupts),
    [messages, pendingInterrupts],
  );
  const useCapsuleComposer =
    composerPresentation === "capsule" || Boolean(workspaceContent);
  const hasFloatingComposerOverlay =
    modeMenuOpen || plusOpen || contextPopoverOpen;
  const capsuleComposerHasRichContent =
    composerManuallyExpanded ||
    attachments.length > 0 ||
    composerContexts.length > 0 ||
    Boolean(composerClarify) ||
    Boolean(realtime.active && realtime.transcript) ||
    agentCreateMissing.length > 0 ||
    welcomeTemplateMissing.length > 0 ||
    fileDragOver;
  const composerLayoutState = useCapsuleComposer
    ? capsuleComposerHasRichContent
      ? "capsule-expanded"
      : "capsule"
    : "default";
  const composerLayoutTransition = reduceComposerMotion
    ? COMPOSER_REDUCED_MOTION_TRANSITION
    : COMPOSER_LAYOUT_TRANSITION;

  useEffect(() => {
    onComposerOverlayOpenChange?.(
      useCapsuleComposer && hasFloatingComposerOverlay,
    );
    return () => onComposerOverlayOpenChange?.(false);
  }, [
    hasFloatingComposerOverlay,
    onComposerOverlayOpenChange,
    useCapsuleComposer,
  ]);
  const canQueueWhileBusy = streaming || turnInFlight || interruptBlocked;
  const canSend =
    !sendBlocked &&
    !modelUnavailable &&
    !realtime.active &&
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
  const modeSwitchLocked =
    streaming || turnInFlight || interruptBlocked || realtime.active;
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
  const welcomeTemplateActive =
    emptyMode === "chat" && Boolean(welcomeTemplateHints?.length);
  const slotTemplateActive = emptyMode === "agent" || welcomeTemplateActive;
  const templateSegments = useMemo(
    () =>
      emptyMode === "agent"
        ? listTemplateSegments(input)
        : welcomeTemplateActive
          ? listPromptTemplateSegments(
              input,
              welcomeTemplateHints ?? [],
              (welcomeTemplateHints ?? []).map((_, index) => index),
            )
          : [],
    [emptyMode, input, welcomeTemplateActive, welcomeTemplateHints],
  );
  const templateMissingSet = useMemo(
    () =>
      new Set(
        emptyMode === "agent" ? agentCreateMissing : welcomeTemplateMissing,
      ),
    [agentCreateMissing, emptyMode, welcomeTemplateMissing],
  );

  useEffect(() => {
    setAgentCreateMissing((prev) => {
      if (emptyMode !== "agent" || prev.length === 0) {
        return prev.length === 0 ? prev : [];
      }
      const still = prepareAgentCreateSend(input).missingRequired.map(
        (s) => s.index,
      );
      if (
        still.length === prev.length &&
        still.every((idx, n) => idx === prev[n])
      ) {
        return prev;
      }
      return still;
    });
  }, [emptyMode, input]);

  useEffect(() => {
    setWelcomeTemplateMissing((previous) => {
      if (!welcomeTemplateActive || previous.length === 0) {
        return previous.length === 0 ? previous : [];
      }
      const still = preparePromptTemplateSend(
        input,
        welcomeTemplateHints ?? [],
      ).missing.map((slot) => slot.index);
      if (
        still.length === previous.length &&
        still.every((index, position) => index === previous[position])
      ) {
        return previous;
      }
      return still;
    });
  }, [input, welcomeTemplateActive, welcomeTemplateHints]);

  useEffect(() => {
    setWelcomeTemplateHints(null);
    setWelcomeTemplateMissing([]);
  }, [sessionId]);

  useEffect(() => {
    if (welcomeTemplateActive && input.length === 0) {
      setWelcomeTemplateHints(null);
      setWelcomeTemplateMissing([]);
    }
  }, [input, welcomeTemplateActive]);

  const effectiveSendMode: ChatSendMode | undefined = showStopControl
    ? sendMode
    : undefined;

  const trySubmitComposer = () => {
    if (!canSend) return;
    const modeOpt = effectiveSendMode;
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
      onSend({
        text: serializeComposerContext(composerContexts, prep.sanitized),
        sendMode: modeOpt,
      });
      onInputChange("");
      setComposerContexts([]);
      setPreviewTarget(null);
      return;
    }
    if (welcomeTemplateActive) {
      const prep = preparePromptTemplateSend(input, welcomeTemplateHints ?? []);
      if (!prep.ok) {
        setWelcomeTemplateMissing(prep.missing.map((slot) => slot.index));
        const first = prep.missing[0];
        const el = textareaRef.current;
        if (first && el) {
          requestAnimationFrame(() => {
            el.focus();
            el.setSelectionRange(first.innerStart, first.innerEnd);
          });
        }
        return;
      }
      setWelcomeTemplateHints(null);
      setWelcomeTemplateMissing([]);
      onSend({
        text: serializeComposerContext(composerContexts, prep.sanitized),
        sendMode: modeOpt,
      });
      onInputChange("");
      setComposerContexts([]);
      setPreviewTarget(null);
      return;
    }
    if (composerContexts.length > 0) {
      onSend({
        text: serializeComposerContext(composerContexts, input),
        sendMode: modeOpt,
      });
      onInputChange("");
    } else {
      onSend({ sendMode: modeOpt });
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

  useEffect(() => {
    if (!welcomeTemplateActive) return;
    const el = textareaRef.current;
    const slot = nextEmptyPromptTemplateSlot(
      input,
      -1,
      welcomeTemplateHints ?? [],
    );
    if (!el || !slot) return;
    requestAnimationFrame(() => {
      el.focus();
      el.setSelectionRange(slot.innerStart, slot.innerEnd);
    });
  }, [welcomeTemplateActive, welcomeTemplateHints]);

  const pickWelcomePrompt = useCallback(
    (prompt: string, slotHints: string[]) => {
      setWelcomeTemplateHints(slotHints);
      setWelcomeTemplateMissing([]);
      onPickWelcomePrompt(prompt);
    },
    [onPickWelcomePrompt],
  );

  const syncSlotMirrorScroll = () => {
    const ta = textareaRef.current;
    const mirror = slotMirrorRef.current;
    if (!ta || !mirror) return;
    mirror.scrollTop = ta.scrollTop;
    mirror.scrollLeft = ta.scrollLeft;
  };

  const composerPlaceholder =
    streaming || turnInFlight
      ? sendMode === "steer"
        ? t("chat.placeholderSteer")
        : sendMode === "interrupt"
          ? t("chat.placeholderInterrupt")
          : t("chat.placeholderStreaming")
      : sendBlocked && sendBlockedReason
        ? sendBlockedReason
        : emptyMode === "chat"
          ? ""
          : attachments.length
            ? t("chat.placeholderWithAttach")
            : workspaceContent
              ? t("chat.placeholderFile")
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
    (message: ConversationEntry) => {
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
    [
      lastUserMessageId,
      onEditUserMessage,
      sendBlocked,
      streaming,
      turnInFlight,
    ],
  );

  const cancelUserMessageEdit = useCallback(() => {
    if (submittingUserEdit) return;
    setEditingUserMessageId(null);
    setEditingUserDraft("");
  }, [submittingUserEdit]);

  const submitUserMessageEdit = useCallback(async () => {
    const messageId = editingUserMessageId;
    const content = editingUserDraft.trim();
    if (!messageId || !content || !onEditUserMessage || submittingUserEdit)
      return;
    setSubmittingUserEdit(true);
    try {
      const accepted = await onEditUserMessage(messageId, content);
      if (!accepted) return;
      setEditingUserMessageId(null);
      setEditingUserDraft("");
    } finally {
      setSubmittingUserEdit(false);
    }
  }, [
    editingUserDraft,
    editingUserMessageId,
    onEditUserMessage,
    submittingUserEdit,
  ]);

  const contextMenuMessage = useMemo(
    () =>
      assistantMenu
        ? (messages.find((message) => message.id === assistantMenu.messageId) ??
          null)
        : null,
    [assistantMenu, messages],
  );

  const openAssistantTurnMenu = useCallback(
    (
      messageId: string,
      x: number,
      y: number,
      respectTextSelection = true,
      returnFocus: HTMLButtonElement | null = null,
    ) => {
      if (respectTextSelection) {
        const selection = window.getSelection();
        if (
          selection &&
          !selection.isCollapsed &&
          selection.toString().trim()
        ) {
          return false;
        }
      }
      assistantMenuReturnFocusRef.current = returnFocus;
      setAssistantMenu({ messageId, x, y });
      return true;
    },
    [],
  );

  const closeAssistantTurnMenu = useCallback((restoreFocus = false) => {
    const returnFocus = assistantMenuReturnFocusRef.current;
    assistantMenuReturnFocusRef.current = null;
    setAssistantMenu(null);
    if (restoreFocus && returnFocus?.isConnected) {
      window.requestAnimationFrame(() => returnFocus.focus());
    }
  }, []);

  useEffect(() => {
    if (
      assistantMenu &&
      !messages.some((message) => message.id === assistantMenu.messageId)
    ) {
      closeAssistantTurnMenu(false);
    }
  }, [assistantMenu, closeAssistantTurnMenu, messages]);

  const copyAssistantTurnText = useCallback(
    async (text: string) => {
      if (!text) return;
      try {
        await navigator.clipboard.writeText(text);
        showToast(t("chat.copied"));
      } catch (error) {
        showToast(error instanceof Error ? error.message : String(error), {
          tone: "error",
        });
      }
    },
    [showToast, t],
  );

  const handleAssistantTurnMenuAction = useCallback(
    (action: AssistantTurnMenuAction) => {
      const menu = assistantMenu;
      if (!menu) return;
      const message = messages.find(
        (candidate) => candidate.id === menu.messageId,
      );
      if (!message || message.role !== "assistant") return;

      if (action === "layout-default") {
        setMessageLayoutOverrides((current) => {
          const next = { ...current };
          delete next[message.id];
          return next;
        });
        return;
      }
      if (action === "layout-timeline" || action === "layout-grouped") {
        const layout = action === "layout-grouped" ? "grouped" : "timeline";
        setMessageLayoutOverrides((current) => ({
          ...current,
          [message.id]: layout,
        }));
        return;
      }
      if (action === "set-layout-default") {
        const layout = messageLayoutOverrides[message.id];
        if (!layout || !onDefaultAnswerLayoutChange) return;
        onDefaultAnswerLayoutChange(layout);
        setMessageLayoutOverrides((current) => {
          const next = { ...current };
          delete next[message.id];
          return next;
        });
        return;
      }
      if (action === "toggle-process") {
        setMessageProcessExpanded((current) => ({
          ...current,
          [message.id]: !(
            current[message.id] ?? displayPrefs.processDefaultOpen
          ),
        }));
        return;
      }
      if (action === "copy-answer") {
        void copyAssistantTurnText(assistantAnswerPlainText(message.content));
        return;
      }
      if (action === "copy-markdown") {
        void copyAssistantTurnText(message.content);
        return;
      }
      if (action === "copy-process") {
        void copyAssistantTurnText(assistantProcessMarkdown(message));
        return;
      }
      onBranchMessage?.(message.id);
    },
    [
      assistantMenu,
      copyAssistantTurnText,
      displayPrefs.processDefaultOpen,
      messageLayoutOverrides,
      messages,
      onBranchMessage,
      onDefaultAnswerLayoutChange,
    ],
  );

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
        <TaskCompletionCelebration trigger={completionCelebrationId} />
        {assistantMenu && contextMenuMessage?.role === "assistant" ? (
          <AssistantTurnContextMenu
            x={assistantMenu.x}
            y={assistantMenu.y}
            defaultLayout={displayPrefs.answerLayout}
            layoutOverride={messageLayoutOverrides[contextMenuMessage.id]}
            processExpanded={
              messageProcessExpanded[contextMenuMessage.id] ??
              displayPrefs.processDefaultOpen
            }
            hasAnswer={Boolean(contextMenuMessage.content.trim())}
            hasProcess={Boolean(
              contextMenuMessage.reasoning?.trim() ||
                contextMenuMessage.activities?.length,
            )}
            canSetDefault={Boolean(onDefaultAnswerLayoutChange)}
            canBranch={!streaming && !turnInFlight && Boolean(onBranchMessage)}
            onAction={handleAssistantTurnMenuAction}
            onClose={closeAssistantTurnMenu}
          />
        ) : null}
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
            onPickCard={pickWelcomePrompt}
            onActivate={() => textareaRef.current?.focus()}
          />
        ) : (
          <div className="message-list-wrap">
            <div
              className="message-list"
              ref={messageListRef}
              onScroll={updateConversationScrollState}
            >
              {messages.map((m, index) => (
                <ChatMessageRow
                  key={m.id}
                  message={m}
                  isLastMessage={index === messages.length - 1}
                  isParallelRunning={parallelRunningIds.has(m.id)}
                  isLastUserMessage={m.id === lastUserMessageId}
                  displayPrefs={displayPrefs}
                  answerLayout={
                    messageLayoutOverrides[m.id] ?? displayPrefs.answerLayout
                  }
                  forcedProcessOpen={messageProcessExpanded[m.id]}
                  streaming={streaming}
                  turnInFlight={turnInFlight}
                  sendBlocked={sendBlocked}
                  isEditingUserMessage={editingUserMessageId === m.id}
                  editingUserDraft={editingUserDraft}
                  submittingUserEdit={submittingUserEdit}
                  pendingAsyncQuestions={pendingAsyncQuestionsAt(
                    messages,
                    index,
                  )}
                  activeAgent={activeAgent}
                  assistantHasCustomAvatar={assistantHasCustomAvatar}
                  modelId={modelId}
                  mediaBaseDir={mediaBaseDir}
                  projectId={projectId}
                  openAssistantTurnMenu={openAssistantTurnMenu}
                  setEditingUserDraft={setEditingUserDraft}
                  beginUserMessageEdit={beginUserMessageEdit}
                  cancelUserMessageEdit={cancelUserMessageEdit}
                  submitUserMessageEdit={submitUserMessageEdit}
                  onEditUserMessage={onEditUserMessage}
                  onBranchMessage={onBranchMessage}
                  onOpenActivityUrl={onOpenActivityUrl}
                  onOpenFileReview={onOpenFileReview}
                  onUiAction={onUiAction}
                  onSend={onSend}
                />
              ))}
              <div ref={bottomRef} />
            </div>
            <TurnNavigator
              messages={messages}
              listRef={messageListRef}
              bottomRef={bottomRef}
            />
          </div>
        )}

        <motion.form
          ref={composerShellRef}
          className={`composer-shell${useCapsuleComposer ? " is-capsule" : ""}${
            useCapsuleComposer && capsuleComposerHasRichContent
              ? " is-capsule-expanded"
              : ""
          }`}
          layout={!reduceComposerMotion}
          layoutDependency={composerLayoutState}
          transition={{ layout: composerLayoutTransition }}
          onSubmit={(e) => {
            e.preventDefault();
            if (composerContexts.length === 0 && tryHandleSlashSubmit()) return;
            trySubmitComposer();
          }}
        >
          {isScrolledFromBottom ? (
            <button
              type="button"
              className={`chat-scroll-latest ${showStopControl ? "is-active" : ""}`.trim()}
              title={t("chat.navScrollBottom")}
              aria-label={t("chat.navScrollBottom")}
              onClick={scrollConversationToBottom}
            >
              {showStopControl ? (
                <span className="chat-scroll-latest-dots" aria-hidden>
                  <span />
                  <span />
                  <span />
                </span>
              ) : (
                <ArrowDown size={18} strokeWidth={2} aria-hidden />
              )}
            </button>
          ) : null}
          <TodoProgress
            messages={messages}
            onOpenFileReview={onOpenFileReview}
          />
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
              aria-label={t("chat.queue.title", {
                count: String(queuedFollowUps.length),
              })}
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
                          {isSteering
                            ? t("chat.queue.steering")
                            : t("chat.queue.steer")}
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
                        onClick={() =>
                          setQueueMenuId((current) =>
                            current === item.id ? null : item.id,
                          )
                        }
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
                          <span
                            className="composer-queue-menu-separator"
                            role="separator"
                          />
                          <button
                            type="button"
                            role="menuitem"
                            onClick={() => {
                              if (onCloseQueuedFollowUps?.())
                                setQueueMenuId(null);
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
                          <span
                            className="composer-task-worktree"
                            title={task.worktree.path}
                          >
                            {task.worktree.path
                              .split(/[/\\]/)
                              .slice(-2)
                              .join("/")}
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

          <motion.div
            data-tour="composer"
            className={`composer composer--stacked ${composerManuallyExpanded ? "is-input-expanded" : ""} ${composerClarify ? "has-clarify" : ""} ${fileDragOver ? "is-file-dragover" : ""}`.trim()}
            layout={!reduceComposerMotion}
            layoutDependency={composerLayoutState}
            transition={{ layout: composerLayoutTransition }}
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
                  <div
                    key={att.id}
                    className="composer-preview"
                    data-kind={att.kind}
                  >
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
                        <span
                          className="composer-preview-icon"
                          data-kind={att.kind}
                        >
                          <AttachmentGlyph kind={att.kind} />
                        </span>
                      )}
                      <span className="composer-preview-meta">
                        <span className="composer-preview-name">
                          {att.name}
                        </span>
                        <span className="composer-preview-size">
                          {att.kind === "folder"
                            ? t("chat.plusMenuFolder")
                            : formatSize(att.size)}
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
                    file: t("chat.contextToken.file"),
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
                        aria-label={t("chat.previewContext", {
                          name: token.name,
                        })}
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
                        aria-label={t("chat.removeContext", {
                          name: token.name,
                        })}
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
            {!composerClarify &&
            emptyMode === "agent" &&
            agentCreateMissing.length > 0 ? (
              <p className="composer-agent-validate-hint" role="alert">
                {t("chat.agentCreateNeedRequired")}
              </p>
            ) : !composerClarify &&
              welcomeTemplateActive &&
              welcomeTemplateMissing.length > 0 ? (
              <p className="composer-agent-validate-hint" role="alert">
                {t("chat.welcomeTemplateNeedRequired")}
              </p>
            ) : null}
            {hasLiveInteraction ? (
              <ConversationInteractions sessionId={sessionId} />
            ) : composerClarify ? (
              <ComposerClarifySurface
                surface={composerClarify.surface}
                mediaBaseDir={mediaBaseDir}
                onAction={(name, context) =>
                  onUiAction?.(composerClarify.messageId, name, context)
                }
              />
            ) : (
              <div
                className={`composer-input-wrap ${slotTemplateActive ? "is-slot-template" : ""}`.trim()}
              >
                <div
                  className="composer-typed-hint"
                  hidden={!typingPlaceholderEnabled}
                  aria-hidden
                >
                  <span ref={typedHintRef} className="composer-typed-text" />
                  <span className="composer-typed-caret" />
                </div>
                {slotTemplateActive ? (
                  <div
                    ref={slotMirrorRef}
                    className="composer-slot-mirror"
                    aria-hidden
                  >
                    {templateSegments.map((seg, i) =>
                      seg.type === "text" ? (
                        <span key={`t-${i}`}>{seg.value}</span>
                      ) : (
                        <span
                          key={`s-${i}`}
                          className={[
                            "composer-slot-chip",
                            seg.empty ? "is-empty" : "is-filled",
                            templateMissingSet.has(seg.index)
                              ? "is-invalid"
                              : "",
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
                  className={`composer-input ${slotTemplateActive ? "is-slot-highlight" : ""}`.trim()}
                  value={input}
                  rows={2}
                  onChange={(e) => {
                    const v = e.target.value;
                    onInputChange(v);
                    syncTriggerFromCaret(
                      v,
                      e.target.selectionStart ?? v.length,
                    );
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
                    if (slotTemplateActive) {
                      const caret = el.selectionStart ?? 0;
                      const slot = welcomeTemplateActive
                        ? findPromptTemplateSlotAt(
                            input,
                            caret,
                            welcomeTemplateHints ?? [],
                          )
                        : findSlotAt(input, caret);
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
                      : welcomeTemplateActive
                        ? t("chat.welcomeTemplateComposerAria")
                        : emptyMode === "chat"
                          ? t("chat.welcomePlaceholder")
                          : composerPlaceholder || t("chat.placeholder")
                  }
                  disabled={sendBlocked || realtime.active}
                  autoFocus
                />
                <button
                  type="button"
                  className="composer-expand-btn"
                  aria-expanded={composerManuallyExpanded}
                  aria-label={
                    composerManuallyExpanded
                      ? t("chat.composerCollapse")
                      : t("chat.composerExpand")
                  }
                  title={
                    composerManuallyExpanded
                      ? t("chat.composerCollapse")
                      : t("chat.composerExpand")
                  }
                  onClick={() => {
                    setComposerManuallyExpanded((expanded) => !expanded);
                    window.requestAnimationFrame(() =>
                      textareaRef.current?.focus(),
                    );
                  }}
                >
                  <svg
                    className="composer-expand-indicator"
                    viewBox="0 0 44 44"
                    aria-hidden
                  >
                    <path d="M 22 11 A 11 11 0 0 1 33 22" pathLength={1} />
                  </svg>
                  <span className="composer-expand-glyph" aria-hidden>
                    <Maximize2
                      className="composer-expand-icon is-expand"
                      size={12}
                      strokeWidth={2.2}
                    />
                    <Minimize2
                      className="composer-expand-icon is-collapse"
                      size={12}
                      strokeWidth={2.2}
                    />
                  </span>
                </button>
              </div>
            )}
            {realtime.active && realtime.transcript ? (
              <div className="composer-realtime-transcript" aria-live="polite">
                {realtime.transcript}
              </div>
            ) : null}
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
                      setSendModeOpen(false);
                      setContextPopoverOpen(false);
                      setPaletteKind(null);
                      setModeMenuOpen((o) => !o);
                    }}
                  >
                    {(() => {
                      const Meta = modeMeta[chatMode];
                      const Icon = Meta.Icon;
                      const ApprovalIcon = approvalMeta[approvalMode].Icon;
                      return (
                        <>
                          <Icon
                            className="composer-mode-current-icon"
                            size={15}
                            strokeWidth={2.2}
                            aria-hidden
                          />
                          <span className="composer-mode-pill-label">
                            {Meta.label}
                          </span>
                          <span
                            className="composer-policy-separator"
                            aria-hidden
                          >
                            ·
                          </span>
                          <ApprovalIcon
                            className="composer-policy-current-icon"
                            size={15}
                            strokeWidth={2.2}
                            aria-hidden
                          />
                          <span className="composer-policy-approval">
                            {approvalMeta[approvalMode].label}
                          </span>
                          <ChevronDown
                            className="composer-mode-chevron"
                            size={14}
                            strokeWidth={2}
                            aria-hidden
                          />
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
                                  <span className="composer-mode-item-label">
                                    {Meta.label}
                                  </span>
                                  <span className="composer-mode-item-desc">
                                    {Meta.desc}
                                  </span>
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
                                  <span className="composer-mode-item-desc">
                                    {Meta.desc}
                                  </span>
                                </span>
                                {selected ? (
                                  <Check size={14} strokeWidth={2.4} />
                                ) : null}
                              </button>
                            );
                          })}
                          {sessionGrants &&
                          (sessionGrants.workspaceWrite ||
                            sessionGrants.writableRoots.length > 0) ? (
                            <div className="composer-policy-grants">
                              <span className="composer-policy-grants-title">
                                {t("chat.approval.sessionGrants")}
                              </span>
                              <ul className="composer-policy-grants-list">
                                {sessionGrants.writableRoots.map((root) => (
                                  <li key={root} title={root}>
                                    {root}
                                  </li>
                                ))}
                              </ul>
                              <button
                                type="button"
                                className="composer-policy-grants-revoke"
                                onClick={() => void revokeSessionGrants()}
                              >
                                {t("chat.approval.revokeSessionGrants")}
                              </button>
                            </div>
                          ) : null}
                          <div
                            className="composer-approval-health"
                            data-status={sandboxHealth?.status ?? "unknown"}
                          >
                            <span
                              className="composer-approval-health-dot"
                              aria-hidden
                            />
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
                    disabled={streaming || realtime.active}
                    title={t("chat.plusMenu")}
                    aria-label={t("chat.plusMenu")}
                    aria-haspopup="dialog"
                    aria-expanded={plusOpen}
                    onClick={() => {
                      setModeMenuOpen(false);
                      setSendModeOpen(false);
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
                    canAttachFolder={attachments.length < MAX_ATTACHMENTS}
                    onAttach={() => fileInputRef.current?.click()}
                    onAttachFolder={() => void addFolder()}
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
                {!showStopControl ? (
                  <button
                    type="button"
                    className={`composer-icon-btn composer-realtime-btn ${
                      realtime.active ? "is-on" : ""
                    } ${realtime.status === "connecting" ? "is-connecting" : ""}`.trim()}
                    disabled={
                      sendBlocked ||
                      !sessionId ||
                      !realtimeAvailable ||
                      !realtimeProviderId ||
                      !realtimeBackendId
                    }
                    aria-pressed={realtime.active}
                    aria-label={t(
                      !realtimeAvailable
                        ? "chat.realtimeUnavailable"
                        : realtime.active
                          ? "chat.realtimeStop"
                          : "chat.realtimeStart",
                    )}
                    title={t(
                      !realtimeAvailable
                        ? "chat.realtimeUnavailable"
                        : realtime.status === "connecting"
                          ? "chat.realtimeConnecting"
                          : realtime.active
                            ? "chat.realtimeStop"
                            : "chat.realtimeStart",
                    )}
                    onClick={realtime.toggle}
                  >
                    {realtime.active ? (
                      <PhoneOff size={17} strokeWidth={2.1} />
                    ) : (
                      <Mic size={17} strokeWidth={2.1} />
                    )}
                  </button>
                ) : null}
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
                    <div className="composer-send-mode" ref={sendModeWrapRef}>
                      <button
                        type="button"
                        className={`composer-mode-pill composer-send-mode-pill ${sendModeOpen ? "is-open" : ""}`}
                        aria-haspopup="menu"
                        aria-expanded={sendModeOpen}
                        aria-label={t("chat.sendMode.menu")}
                        title={sendModeMeta[sendMode].label}
                        onClick={() => {
                          setModeMenuOpen(false);
                          setPlusOpen(false);
                          setContextPopoverOpen(false);
                          setSendModeOpen((o) => !o);
                        }}
                      >
                        {(() => {
                          const Meta = sendModeMeta[sendMode];
                          return (
                            <>
                              <span className="composer-mode-pill-label">
                                {Meta.label}
                              </span>
                              <ChevronDown
                                className="composer-mode-chevron"
                                size={14}
                                strokeWidth={2}
                                aria-hidden
                              />
                            </>
                          );
                        })()}
                      </button>
                      {sendModeOpen && typeof document !== "undefined"
                        ? createPortal(
                            <div
                              ref={sendModePanelRef}
                              className="composer-mode-menu composer-send-mode-menu"
                              role="menu"
                              style={sendModeStyle ?? { visibility: "hidden" }}
                            >
                              {CHAT_SEND_MODES.map((mode) => {
                                const Meta = sendModeMeta[mode];
                                const Icon = Meta.Icon;
                                const selected = mode === sendMode;
                                return (
                                  <button
                                    key={mode}
                                    type="button"
                                    role="menuitemradio"
                                    aria-checked={selected}
                                    className={`composer-mode-item ${selected ? "is-selected" : ""}`}
                                    onClick={() => {
                                      setSendMode(mode);
                                      saveChatSendMode(mode);
                                      setSendModeOpen(false);
                                    }}
                                  >
                                    <Icon size={16} strokeWidth={2} />
                                    <span className="composer-mode-item-text">
                                      <span className="composer-mode-item-label">
                                        {Meta.label}
                                      </span>
                                      <span className="composer-mode-item-desc">
                                        {Meta.desc}
                                      </span>
                                    </span>
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
                    {showPauseResume ? (
                      <button
                        type="button"
                        className="composer-icon-btn"
                        onClick={() =>
                          streamPaused ? onResumeStream?.() : onPauseStream?.()
                        }
                        title={t(
                          streamPaused
                            ? "chat.streamResume"
                            : "chat.streamPause",
                        )}
                        aria-label={t(
                          streamPaused
                            ? "chat.streamResume"
                            : "chat.streamPause",
                        )}
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
                    {canSend ? (
                      <button
                        className="send-btn send-btn--round"
                        type="submit"
                        aria-label={t("chat.send")}
                        title={`${sendModeMeta[sendMode].label}: ${sendModeMeta[sendMode].desc}`}
                      >
                        <ArrowUp size={19} strokeWidth={2.2} aria-hidden />
                      </button>
                    ) : null}
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
                    <ArrowUp size={19} strokeWidth={2.2} aria-hidden />
                  </button>
                ) : null}
              </div>
            </div>
          </motion.div>
        </motion.form>
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
          onToggleArchived={() => void toggleCronJobArchived()}
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
