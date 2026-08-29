/**
 * 前端共享类型：聊天、供应商、技能、产物等与 Tauri/Rust 序列化对齐的 DTO。
 */

/** 聊天区空状态：欢迎卡片、创建 Agent 引导，或正常消息列表 */
export type ChatEmptyMode = "chat" | "agent" | null;

/** DeepSeek / OpenRouter 等模型的推理力度 */
export type ReasoningEffort =
  | "none"
  | "minimal"
  | "low"
  | "medium"
  | "high"
  | "xhigh"
  | "max";

/** OpenRouter `reasoning` 对象（档位 / 默认开关） */
export type ModelReasoningMeta = {
  supported_efforts?: string[];
  default_effort?: string | null;
  default_enabled?: boolean | null;
  mandatory?: boolean | null;
  supports_max_tokens?: boolean | null;
};

/** 聊天附件媒体类型 */
export type ChatAttachmentKind = "image" | "video" | "audio" | "file";

/** 用户消息附件 */
export type ChatAttachment = {
  id: string;
  name: string;
  mime: string;
  kind: ChatAttachmentKind;
  size: number;
  /** 本地预览用 object URL（仅前端） */
  previewUrl?: string;
  /** 发送给后端的 base64（图片/小文本） */
  dataBase64?: string;
  /** 本地文件绝对路径（拖放/粘贴来源），大图无 base64 时供后端定位文件 */
  localPath?: string;
};

/** 聊天活动条类型 */
export type ChatActivityKind = "tool" | "skill" | "mcp" | "hook" | "memory" | "status";

/** 助手气泡旁的活动记录 */
export type ChatActivity = {
  id: string;
  kind: ChatActivityKind;
  title: string;
  detail?: string;
  /** 工具 arguments / 调用入参 */
  input?: string;
  /** 工具 result / 记忆 content */
  output?: string;
  status?: "running" | "done" | "error" | "interrupted";
  /** 开始时刻（ms） */
  at?: number;
  /** 调用耗时（秒），完成态写入 */
  durationSec?: number;
  /** 结构化媒体（优先于从 output 文本 regex 解析） */
  media?: Array<{
    kind: "image" | "video" | "audio" | "html" | "code";
    path: string;
  }>;
};

/** A2UI surface 生命周期 */
export type UiSurfaceStatus = "active" | "resolved" | "cancelled";

/** 聊天气泡内的声明式 UI 表面（AG-UI activity / A2UI） */
export type UiSurface = {
  messageId: string;
  activityType: string;
  operations: unknown[];
  status: UiSurfaceStatus;
  interrupts?: Array<{
    id: string;
    reason: string;
    message?: string;
    responseSchema?: unknown;
  }>;
};

/** 会话级未决 HITL interrupt */
export type PendingInterrupt = {
  id: string;
  reason: string;
  message?: string;
  responseSchema?: unknown;
  /** 所属助手消息 id */
  assistantMessageId?: string;
};

/** 单条助手回复的 token 用量（来自流式 usage 事件） */
export type MessageTokenUsage = {
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
};

/** 聊天列表中的用户或助手消息 */
export type ChatMessage = {
  id: string;
  role: "user" | "assistant";
  content: string;
  /** Non-blocking assistant update emitted while the turn continues. */
  delivery?: "async";
  /** DeepSeek 等 thinking 模式下的推理过程 */
  reasoning?: string;
  /** 思考耗时（秒），用于折叠头展示 */
  reasoningDurationSec?: number;
  /** 本轮生成起点（ms，助手气泡创建时），流式时供实时秒表 */
  generationStartedAt?: number;
  /** 本轮墙钟耗时（秒）：发起→结束，对齐 Hermes TUI 回合计时 */
  generationDurationSec?: number;
  /** 本轮流式 usage，挂在助手消息上供气泡角标展示 */
  usage?: MessageTokenUsage;
  /**
   * 生成速度（tokens/秒）：completion_tokens / 生成耗时。
   * 耗时优先取首 token→结束，否则取开始流式→结束。
   */
  tokensPerSec?: number;
  error?: boolean;
  attachments?: ChatAttachment[];
  activities?: ChatActivity[];
  /** Anthropic citations（引用信息） */
  citations?: Array<Record<string, unknown>>;
  /** A2UI / AG-UI activity 表面 */
  uiSurfaces?: UiSurface[];
  /**
   * 本轮事件时间线（思考 / 工具 / A2UI 交错）。
   * activity/surface 段只存 id，实体在 activities / uiSurfaces。
   */
  segments?: ChatTimelineSegment[];
  createdAt?: number;
};

/** 助手气泡内可交错的时间线段，顺序与模型事件到达顺序一致。 */
export type ChatTimelineSegment =
  | {
      type: "reasoning";
      id: string;
      text: string;
      at: number;
      durationSec?: number;
    }
  | {
      type: "text";
      id: string;
      text: string;
      at: number;
    }
  | {
      type: "activity";
      id: string;
      at: number;
    }
  | {
      type: "surface";
      id: string;
      at: number;
    };

/** `get_chat_history` 返回的活动条（已折叠进助手消息） */
export type ChatHistoryActivityDto = {
  id: string;
  kind: string;
  title: string;
  input?: string | null;
  output?: string | null;
  status?: string | null;
  /** 结构化媒体（来自 messages.media_json）；缺省时前端可从 output 解析 */
  media?: Array<{ kind: string; path: string }> | null;
};

/** `get_chat_history` 单条气泡（user / assistant，含 reasoning + activities） */
export type ChatHistoryMessageDto = {
  id: string;
  role: string;
  content: string;
  reasoning?: string | null;
  activities?: ChatHistoryActivityDto[];
  segments?: ChatTimelineSegment[] | null;
  uiSurfaces?: UiSurface[] | null;
};

/** `get_chat_history` 整包响应 */
export type ChatHistoryDto = {
  sessionId: string | null;
  messages: ChatHistoryMessageDto[];
  /** 会话结束原因，如 `compacted`；未结束为 null */
  endReason?: string | null;
  /** 结束时间（epoch 秒）；未结束为 null */
  endedAt?: number | null;
  /** 临时 Side 会话；离开时自动丢弃 */
  ephemeral?: boolean;
  parentSessionId?: string | null;
  /** UI 隐藏、但模型仍继承的 turn 数 */
  excludedTurnCount?: number;
};

/** 聊天后备链条目（写入 providers.json） */
export type ProviderFallbackEntry = {
  provider_id: string;
  model?: string | null;
};

/** 模型供应商配置（Providers 面板） */
export type ProviderDto = {
  id: string;
  kind: string;
  display_name: string;
  endpoint: string;
  model: string;
  enabled: boolean;
  has_api_key: boolean;
  key_source: "keyring" | "env" | "none" | "not_required" | string;
  env_key_name: string | null;
  backend_id: string;
  official_key_url?: string | null;
  /** 显式聊天后备链（最多 3；缺省为空） */
  fallback?: ProviderFallbackEntry[];
  /** 生图模型（空=内置默认） */
  image_model?: string;
  /** 生视频模型（空=内置默认） */
  video_model?: string;
  /** 生音频 / TTS 模型（空=内置默认） */
  tts_model?: string;
  /** 音乐生成模型（空=内置默认；Google only） */
  music_model?: string;
  /** 视觉（图片理解）模型（空=内置默认） */
  vision_model?: string;
  /** ASR 模型 */
  asr_model?: string;
  supports_image?: boolean;
  supports_video?: boolean;
  supports_tts?: boolean;
  supports_music?: boolean;
  supports_asr?: boolean;
  supports_embedding?: boolean;
  embedding_model?: string;
  /** 当前 API 协议模式（chat_completions / responses / anthropic_messages 等） */
  api_mode?: string;
  /** 是否支持 Responses API 模式切换 */
  supports_responses_api?: boolean;
  /** 配置来源：builtin / toml / user */
  config_source?: string;
};

/** 全部供应商 + 当前激活 id */
export type ProvidersStateDto = {
  providers: ProviderDto[];
  active_provider_id: string | null;
};

/** 辅助模型任务 id（对齐 `memory::AuxiliaryKind` / `common::AuxiliaryTask`） */
export type AuxiliaryTaskId =
  | "title_generation"
  | "compaction"
  | "smart_approval"
  | "dreaming"
  | "background_review"
  | "workflow_ai_polish";

/** 单个辅助任务在设置面的展示态（Tauri `get_auxiliary_settings`） */
export type AuxiliaryTaskDto = {
  id: AuxiliaryTaskId;
  provider: string;
  model: string;
  displayLabel: string;
  unavailable: boolean;
};

/** 五类辅助任务 + 当前激活主模型（供「auto」展示参照） */
export type AuxiliarySettingsDto = {
  tasks: AuxiliaryTaskDto[];
  activeProviderId: string | null;
  activeModel: string;
};

/** 上下文卫生设置（Tauri `get_compression_settings`，对齐 `memory::CompressionConfig`） */
export type CompressionSettingsDto = {
  enabled: boolean;
  softRatio: number;
  mediumRatio: number;
  hardRatio: number;
  softMaxChars: number;
  softHeadChars: number;
  softTailChars: number;
  mediumMaxChars: number;
  mediumHeadChars: number;
  mediumTailChars: number;
  hardMaxChars: number;
  hardHeadChars: number;
  hardTailChars: number;
  toolResultsLimit: number;
  midRunSummaryRatio: number;
  recommendCompactRatio: number;
  protectLastN: number;
  protectFirstMessages: number;
  thrashingMinGainRatio: number;
  thrashingMaxConsecutive: number;
  keepTailBubbles: number;
};

/** 离线进化路由 id（对齐 `memory::EvolutionRouteKind`） */
export type EvolutionRouteId = "reflection" | "judge";

/** 单条进化路由展示态（Tauri `get_evolution_settings`） */
export type EvolutionRouteDto = {
  id: EvolutionRouteId;
  provider: string;
  model: string;
  displayLabel: string;
  unavailable: boolean;
};

/** 进化门禁展示态 */
export type EvolutionGatesDto = {
  runTests: boolean;
  maxSkillBytes: number;
  requirePr: boolean;
  minJudgeScore: number;
};

/** 遗传搜索参数 */
export type EvolutionSearchDto = {
  generations: number;
  variants: number;
  crossover: boolean;
  populationSize: number;
  maxEvalExamples: number;
  maxLlmCalls: number;
  /** [P0] 批准后冷却期（秒）。0 = 禁用。 */
  postApprovalCooldownSecs: number;
};

/** 自动触发参数 */
export type EvolutionAutoDto = {
  enabled: boolean;
  cooldownSecs: number;
  minNewDecisions: number;
  maxRunsPerDay: number;
  /** [P2] 触发定向进化所需的最少失败信号数 */
  minSkillFailureSignals: number;
  /** [P2] 失败信号统计窗口（天） */
  signalWindowDays: number;
};

/** 技能策展参数 */
export type EvolutionCuratorDto = {
  enabled: boolean;
  intervalDays: number;
  maxEnqueue: number;
  llmDiagnose: boolean;
  maxLlmCalls: number;
};

/** 自动触发运行时状态（护栏水位） */
export type EvolutionAutoStatusDto = {
  enabled: boolean;
  cooldownSecs: number;
  minNewDecisions: number;
  maxRunsPerDay: number;
  state: {
    lastRunAt?: string | null;
    lastDecisionId?: string | null;
    runsToday: number;
    runsTodayDate?: string | null;
  };
  newDecisions: number;
  wouldRun: boolean;
  skipReason: string | null;
  skipMessage: string | null;
};

/** 进化历史聚合 + 近期事件（Tauri `evolution_history`） */
export type EvolutionHistoryDto = {
  summary: {
    totalRuns: number;
    runsByMode: Record<string, number>;
    totalGenerated: number;
    totalProposals: number;
    approved: number;
    rejected: number;
    branched: number;
    adoptionRate: number;
    avgAdoptedScore: number;
    scoreTrend: number[];
  };
  recent: Array<Record<string, unknown>>;
};

/** DSPy 对接状态（Tauri `evolution_dspy_status`） */
export type DspyStatusDto = {
  enabled: boolean;
  pythonBin: string;
  pythonOk: boolean;
  projectPath: string | null;
  dspyInstalled: boolean;
  timeoutSecs: number;
};

/** 评测例子（Tauri `list_eval_examples`） */
export type EvalExampleDto = {
  id: string;
  skillId: string | null;
  task: string;
  expectations: string[];
  verdict: "pass" | "fail";
  sourceSession: string | null;
  createdAt: string;
};

/** 可导入的失败会话候选（Tauri `list_eval_import_candidates`） */
export type EvalImportCandidateDto = {
  sessionId: string;
  task: string;
  expectations: string[];
  failCount: number;
};

/** GEPA-lite 遗传搜索运行结果（Tauri `run_evolution_search`） */
export type EvolutionSearchReport = {
  ok: boolean;
  generations: number;
  variantsEvaluated: number;
  paretoKept: number;
  proposals: EvolutionProposalDto[];
  budgetUsed: number;
  holdoutEnabled: boolean;
  sandboxUsed: boolean;
  sandboxSkills: number;
  focusSkill: string | null;
  termination: string;
  error: string | null;
};

/** 搜索每代实时进度事件（Tauri `evolution-search-progress`） */
export type SearchProgressEvent = {
  seedSkill: string;
  seedIndex: number;
  seedTotal: number;
  generation: number;
  generationTotal: number;
  populationScores: number[];
  populationBest: number;
  populationSize: number;
  variantsEvaluated: number;
  budgetUsed: number;
  budgetLimit: number;
  gatedOut: number;
  judgedOut: number;
  critiques: string[];
  /** [P3] 本代运行时机会 hints */
  hints?: Array<{ tag: string; focus: string }>;
};

/** [P1] Skill 快照元数据 */
export type SkillSnapshot = {
  timestamp: string;
  preview: string;
  bytes: number;
};

/** [P2] Skill 失败信号摘要 */
export type SkillSignalDto = {
  skillId: string;
  failureSignals: number;
};

/** 技能策展报告（Tauri `run_skill_curator`） */
export type CurateReportDto = {
  generatedAt: string;
  unusedSkillDays: number;
  enabledCount: number;
  stale: string[];
  overlapClusters?: string[][];
  rows: Array<{
    skillId: string;
    description: string;
    lastLoaded: string | null;
    stale: boolean;
    healthScore: number | null;
    healthReasons: string[];
    bytes: number | null;
  }>;
  suggestions: Array<{
    kind: string;
    skillId: string;
    reason: string;
    absorb?: string[];
  }>;
};

/** 策展调度状态（`curator_status`） */
export type CuratorStatusDto = {
  enabled: boolean;
  intervalDays: number;
  due: boolean;
  daysSinceLast: number | null;
  lastGeneratedAt: string | null;
  suggestionCount: number;
  skipReason: string | null;
  skipMessage: string | null;
};

/** 离线进化设置全量（enabled + reflection/judge 路由 + gates） */
export type EvolutionSettingsDto = {
  enabled: boolean;
  routes: EvolutionRouteDto[];
  gates: EvolutionGatesDto;
  search: EvolutionSearchDto;
  auto: EvolutionAutoDto;
  curator: EvolutionCuratorDto;
  activeProviderId: string | null;
  activeModel: string;
};

/** 单条进化提案（待审） */
export type EvolutionProposalDto = {
  id: string;
  kind: "new_skill" | "patch" | "disable" | "merge";
  skillId: string;
  description: string | null;
  content: string | null;
  oldString: string | null;
  newString: string | null;
  rationale: string;
  judgeScore: number | null;
  judgeReason: string | null;
  createdAt: string;
};

/** 运行一次进化的结果 */
export type EvolutionRunReport = {
  ok: boolean;
  generated: number;
  gatedOut: number;
  judgedOut: number;
  proposals: EvolutionProposalDto[];
  error: string | null;
};

/** 应用图标变体 id（对齐 `app_icon::VARIANTS`） */
export type AppIconId = "blue" | "deep_blue" | "black" | "white" | "white_logo";

/** 单个图标变体（含 base64 缩略图） */
export type AppIconOptionDto = {
  id: AppIconId;
  dataUrl: string;
};

/** 应用图标设置（当前变体 + 可选项）（Tauri `get_app_icon`） */
export type AppIconSettingsDto = {
  current: AppIconId;
  options: AppIconOptionDto[];
};

/** 拉取模型列表结果 */
export type ProviderModelsResult = {
  models: ModelInfo[];
  latency_ms: number;
  source: string;
};

/** 供应商连通性探测结果 */
export type ProviderTestResult = {
  ok: boolean;
  latency_ms: number;
  model: string;
  message: string;
};

/** UI 侧供应商 kind 枚举 */
export type ProviderKindId =
  | "openai"
  | "anthropic"
  | "deepseek"
  | "ollama"
  | "google"
  | "azure"
  | "zhipu"
  | "openrouter"
  | "bailian"
  | "nvidia"
  | "moonshot"
  | "volcengine"
  | "minimax"
  | "hunyuan"
  | "custom";

/** 模型能力位 */
export type ModelCapabilities = {
  vision: boolean;
  web: boolean;
  reasoning: boolean;
  tools: boolean;
  /** 接受 file 输入 */
  file: boolean;
  /** 接受 audio 输入 */
  audio_in: boolean;
  image_gen: boolean;
  video_gen: boolean;
  audio_gen: boolean;
  music_gen: boolean;
};

/** OpenRouter 单价（USD / 百万 tokens） */
export type ModelPricingMeta = {
  prompt_per_million?: number | null;
  completion_per_million?: number | null;
  cache_read_per_million?: number | null;
  cache_write_per_million?: number | null;
};

/** OpenRouter default_parameters */
export type ModelDefaultParams = {
  temperature?: number | null;
  top_p?: number | null;
  top_k?: number | null;
  frequency_penalty?: number | null;
  presence_penalty?: number | null;
  repetition_penalty?: number | null;
};

/** 模型元信息（列表 / 选择器） */
export type ModelInfo = {
  id: string;
  display_name?: string | null;
  description?: string | null;
  canonical_slug?: string | null;
  knowledge_cutoff?: string | null;
  expiration_date?: string | null;
  /** Unix 秒：模型条目创建时间（OpenRouter） */
  created?: number | null;
  hugging_face_id?: string | null;
  is_moderated?: boolean | null;
  context_window?: number | null;
  max_output_tokens?: number | null;
  capabilities: ModelCapabilities;
  /** OpenRouter 推理档位 / 默认开关 */
  reasoning?: ModelReasoningMeta | null;
  pricing?: ModelPricingMeta | null;
  default_parameters?: ModelDefaultParams | null;
  meta_source?: string;
};

/** 记忆召回快照 */
export type MemorySnapshot = {
  memory_content: string;
  user_content: string;
  sessions: { session_id: string; summary: string }[];
};

/** 文件空间 / 工作区目录项 */
export type FileEntryDto = {
  path: string;
  name: string;
  is_dir: boolean;
  size: number;
};

/** Skill 商店来源 id */
export type SkillStoreId = "skillhub" | "skillsdotsh" | "clawhub";

/** 本机已安装 Skill */
export type InstalledSkill = {
  id: string;
  name: string;
  description: string;
  path: string;
  source_dir: string;
  enabled: boolean;
  scope?: "astro" | "machine" | string;
  linked?: boolean;
  provenance?: "packaged" | "user" | "agent" | "project" | "external" | string;
  editable?: boolean;
  shadowed_by?: string | null;
};

/** 商店搜索结果条目 */
export type StoreSkill = {
  id: string;
  name: string;
  description: string;
  source: string;
  store: string;
  installs: number | null;
  install_ref: string;
  homepage: string | null;
};

/** 商店详情 */
export type StoreSkillDetail = {
  name: string;
  slug: string;
  description: string;
  overview: string;
  source: string;
  store: string;
  installs: number | null;
  downloads: number | null;
  stars: number | null;
  install_ref: string;
  homepage: string | null;
  detail_url: string;
  icon_url: string | null;
  category: string | null;
  sub_categories: string[];
  version: string | null;
  updated_at: number | null;
  owner_name: string | null;
  verified: boolean | null;
};

/** 加载后的 Skill 正文 */
export type SkillContent = {
  metadata: {
    name: string;
    description: string;
    version: string;
  };
  content: string;
};

/** 技能包内单个文件 */
export type SkillFileEntry = {
  relative_path: string;
  category: "overview" | "scripts" | "references" | "assets" | "other" | string;
  is_text: boolean;
  size: number;
};

/** 技能包文件清单（查看抽屉） */
export type SkillBundle = {
  name: string;
  description: string;
  root: string;
  files: SkillFileEntry[];
};

/** 技能安装来源记录（`skill-origins.json` 单条，与 Rust `SkillOriginRecord` 对齐） */
export type SkillOriginRecord = {
  folder: string;
  skill_id?: string | null;
  name: string;
  store: string;
  install_ref: string;
  agent_id?: string | null;
  scope?: "astro" | "machine" | string | null;
  installed_at: number;
  last_updated_at?: number | null;
  remote_version?: string | null;
  remote_updated_at?: number | null;
  content_digest?: string | null;
};

/** 更新前本地改动预览（与 Rust `SkillUpdatePreview` 对齐） */
export type SkillUpdatePreview = {
  folder: string;
  has_local_changes: boolean;
  has_baseline_digest: boolean;
  current_digest: string | null;
  baseline_digest: string | null;
};

/** 批量更新单条结果（「更新」Tab，与 Rust `SkillUpdateItemResult` 对齐） */
export type SkillUpdateItemResult = {
  folder: string;
  ok: boolean;
  message: string;
};

/** 技能更新检查状态（与 Rust `SkillUpdateStatus` 对齐，serde snake_case） */
export type SkillUpdateStatus = "outdated" | "current" | "unknown" | "error";

/** 单条技能更新检查结果（与 Rust `SkillUpdateCheckResult` 对齐） */
export type SkillUpdateCheckResult = {
  folder: string;
  status: SkillUpdateStatus;
  remote_version: string | null;
  remote_updated_at: number | null;
  message: string;
};

/** 技能更新本地备份条目（与 Rust `SkillBackupEntry` 对齐） */
export type SkillBackupEntry = {
  agent_id: string;
  folder: string;
  timestamp: string;
  path: string;
  created_at: number | null;
};

/** 「更新」Tab 筛选：v2 中 `updatable` 仅含 `outdated` */
export type SkillUpdateFilter = "with_origin" | "no_origin" | "updatable";

/** 「更新」Tab 合并行：已安装技能 + 可选来源记录 + 远端检查状态 */
export type SkillUpdateRow = {
  skill: InstalledSkill;
  origin: SkillOriginRecord | null;
  status:
    | "no_origin"
    | "with_origin"
    | "outdated"
    | "current"
    | "unknown"
    | "error";
};

/** 侧栏近期会话 */
export type RecentSessionDto = {
  sessionId: string;
  summary: string;
  createdAt: string | null;
  endReason?: string | null;
  archivedAt?: string | null;
  pinnedAt?: string | null;
};

/** 分支画布中的节点。聊天 fork 与 Agent spawn 通过 edgeKind 严格区分。 */
export type BranchGraphNodeDto = {
  id: string;
  kind: "turn" | "branchHead" | "agent";
  sessionId: string;
  parentId: string | null;
  edgeKind: "continuation" | "fork" | "side" | "spawn" | null;
  title: string;
  preview: string;
  status: string;
  createdAt: string | null;
  sourceMessageId: number | null;
  turnIndex: number | null;
  model: string | null;
  agentPath: string | null;
  isCurrent: boolean;
  canFork: boolean;
  isEphemeral: boolean;
  /** turn 节点的完整用户输入；用于在此轮前分支时回填输入框 */
  userMessage?: string | null;
};

/** 当前会话所在整棵聊天谱系，以及附着的子 Agent 执行层。 */
export type BranchGraphDto = {
  rootSessionId: string;
  currentSessionId: string;
  nodes: BranchGraphNodeDto[];
  branchCount: number;
  turnCount: number;
  agentCount: number;
  sideCount: number;
};

/** 后端持久化的项目实体 */
export type ProjectDto = {
  id: string;
  name: string;
  icon?: string | null;
  roots: string[];
  position: number;
  createdAt: string;
  updatedAt: string;
};

/** 产物分类筛选 */
export type ArtifactCategory =
  | "all" | "doc" | "sheet" | "image" | "av" | "code" | "pdf_ppt" | "other";

/** 单条产物记录 */
export type ArtifactDto = {
  id: string;
  path: string;
  name: string;
  category: string;
  mime: string | null;
  size: number;
  source: string;
  session_id: string | null;
  message_id: string | null;
  agent_id: string;
  created_at: string;
  missing: boolean;
};

/** 按会话分组的产物 */
export type ArtifactSessionGroupDto = {
  session_id: string | null;
  session_title: string;
  files: ArtifactDto[];
};

/** list_artifacts 返回 */
export type ListArtifactsResult = {
  groups: ArtifactSessionGroupDto[];
  counts: Record<string, number>;
  total: number;
};
