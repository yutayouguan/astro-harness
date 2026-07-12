/**
 * 前端共享类型：聊天、供应商、技能、产物等与 Tauri/Rust 序列化对齐的 DTO。
 */

/** 聊天区空状态：欢迎卡片、创建 Agent 引导，或正常消息列表 */
export type ChatEmptyMode = "chat" | "agent" | null;

/** DeepSeek 等模型的推理力度 */
export type ReasoningEffort = "high" | "max";

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
};

/** 聊天活动条类型 */
export type ChatActivityKind = "tool" | "skill" | "mcp" | "hook" | "memory" | "status";

/** 助手气泡旁的活动记录 */
export type ChatActivity = {
  id: string;
  kind: ChatActivityKind;
  title: string;
  detail?: string;
  status?: "running" | "done" | "error";
  at?: number;
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
  /** DeepSeek 等 thinking 模式下的推理过程 */
  reasoning?: string;
  /** 思考耗时（秒），用于折叠头展示 */
  reasoningDurationSec?: number;
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
  createdAt?: number;
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
};

/** 全部供应商 + 当前激活 id */
export type ProvidersStateDto = {
  providers: ProviderDto[];
  active_provider_id: string | null;
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
  | "custom";

/** 模型能力位 */
export type ModelCapabilities = {
  vision: boolean;
  web: boolean;
  reasoning: boolean;
  tools: boolean;
};

/** 模型元信息（列表 / 选择器） */
export type ModelInfo = {
  id: string;
  display_name?: string | null;
  context_window?: number | null;
  max_output_tokens?: number | null;
  capabilities: ModelCapabilities;
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
export type SkillStoreId = "skillhub" | "skillsdotsh";

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

/** 侧栏近期会话 */
export type RecentSessionDto = {
  sessionId: string;
  summary: string;
  createdAt: string | null;
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
