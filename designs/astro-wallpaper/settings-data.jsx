const SETTINGS_GROUPS = [
  {
    id: "general",
    label: "通用",
    items: [
      { id: "preferences", label: "偏好设置", icon: "◇", eyebrow: "GENERAL", description: "管理语言、启动方式与日常行为。" },
      { id: "appearance", label: "外观", icon: "◐", eyebrow: "APPEARANCE", description: "调整主题、背景、材质与图标风格。" },
      { id: "conversation", label: "对话", icon: "◍", eyebrow: "CONVERSATION", description: "决定回答展示密度与运行过程的可见性。" },
      { id: "terminal", label: "终端", icon: "›_", eyebrow: "TERMINAL", description: "设置执行模式、字体与会话行为。" },
    ],
  },
  {
    id: "intelligence",
    label: "智能体",
    items: [
      { id: "context", label: "自动压缩", icon: "▤", eyebrow: "CONTEXT", description: "在质量和上下文成本之间设定自动压缩策略。" },
      { id: "providers", label: "模型服务", icon: "◎", eyebrow: "PROVIDERS", description: "连接模型供应商，并管理认证和默认路由。" },
      { id: "tools", label: "工具与技能", icon: "✧", eyebrow: "TOOLS & SKILLS", description: "控制智能体可以使用的本地工具、Skills 与 MCP。" },
      { id: "memory", label: "记忆", icon: "◌", eyebrow: "MEMORY", description: "查看长期记忆、用户画像与待审核条目。" },
    ],
  },
  {
    id: "extensions",
    label: "扩展",
    items: [
      { id: "browser", label: "浏览器", icon: "⊙", eyebrow: "BROWSER", description: "配置浏览器运行方式、启动页和站点权限。" },
      { id: "models", label: "模型市场", icon: "⬡", eyebrow: "MODEL MARKET", description: "浏览模型能力，并将选定模型配置到任务。" },
      { id: "insights", label: "数据洞察", icon: "◫", eyebrow: "INSIGHTS", description: "分析调用量、成本、工具使用和协作轨迹。" },
    ],
  },
  {
    id: "system",
    label: "系统",
    items: [
      { id: "diagnostics", label: "诊断", icon: "△", eyebrow: "DIAGNOSTICS", description: "检查运行日志、组件状态与本地环境。" },
      { id: "about", label: "关于 Astro", icon: "ⓘ", eyebrow: "ABOUT", description: "查看版本、更新状态与项目信息。" },
    ],
  },
];

const ALL_TABS = SETTINGS_GROUPS.flatMap((group) => group.items);

const WALLPAPERS = {
  valley: { id: "valley", name: "柔光山谷", src: "assets/valley-light.jpg" },
  iridescence: { id: "iridescence", name: "虹彩流光", src: "assets/iridescence-light.jpg" },
};

const PROVIDERS = [
  { id: "openai", mark: "OA", name: "OpenAI", detail: "Responses API · 8 个模型", status: "connected" },
  { id: "openrouter", mark: "OR", name: "OpenRouter", detail: "Responses · 自动模型目录", status: "connected" },
  { id: "google", mark: "G", name: "Google Gemini", detail: "Interactions API · 多模态", status: "connected" },
  { id: "deepseek", mark: "DS", name: "DeepSeek", detail: "Responses · 原生工具", status: "setup" },
];

const TOOLS = [
  { id: "terminal", mark: ">_", name: "终端", detail: "运行命令与后台任务", scope: "系统", enabled: true },
  { id: "files", mark: "▧", name: "文件", detail: "读写工作区文件", scope: "工作区", enabled: true },
  { id: "browser", mark: "◎", name: "浏览器", detail: "打开、操作与检查网页", scope: "扩展", enabled: true },
  { id: "media", mark: "◈", name: "媒体生成", detail: "图像、语音、视频与音乐", scope: "媒体", enabled: false },
  { id: "memory", mark: "◌", name: "记忆", detail: "读取与更新长期记忆", scope: "智能体", enabled: true },
];

const MODELS = [
  { id: "gpt-5.6", name: "GPT-5.6", provider: "OpenAI", type: "generation", caps: ["Tools", "Vision", "Reasoning"], context: "400K" },
  { id: "gemini-3.5-pro", name: "Gemini 3.5 Pro", provider: "Google", type: "generation", caps: ["Vision", "Long context"], context: "1M" },
  { id: "deepseek-v4", name: "DeepSeek V4", provider: "DeepSeek", type: "generation", caps: ["Tools", "Reasoning"], context: "128K" },
  { id: "claude-opus-5", name: "Claude Opus 5", provider: "OpenRouter", type: "generation", caps: ["Tools", "Vision"], context: "200K" },
  { id: "gpt-image-2", name: "GPT Image 2", provider: "Azure", type: "image", caps: ["Image"], context: "—" },
  { id: "veo-3", name: "Veo 3", provider: "Google", type: "video", caps: ["Video"], context: "—" },
  { id: "gemini-tts", name: "Gemini TTS", provider: "Google", type: "speech", caps: ["Speech"], context: "32K" },
  { id: "whisper-1", name: "Whisper 1", provider: "OpenAI", type: "transcription", caps: ["Audio"], context: "—" },
  { id: "lyria", name: "Lyria", provider: "Google", type: "music", caps: ["Music"], context: "—" },
  { id: "text-embedding-3", name: "Text Embedding 3", provider: "OpenAI", type: "embedding", caps: ["Embedding"], context: "8K" },
  { id: "cohere-rerank", name: "Rerank 3.5", provider: "Cohere", type: "rerank", caps: ["Rerank"], context: "4K" },
  { id: "gemini-flash", name: "Gemini Flash", provider: "Google", type: "generation", caps: ["Fast", "Vision"], context: "1M" },
];

const LOG_ROWS = [
  { time: "10:42:18", level: "info", source: "agent", message: "turn completed · 7.3s · 12,840 tokens" },
  { time: "10:42:14", level: "info", source: "tools", message: "terminal finished successfully" },
  { time: "10:41:58", level: "warn", source: "provider", message: "primary stream retried before first chunk" },
  { time: "10:41:43", level: "info", source: "memory", message: "workspace snapshot refreshed" },
  { time: "10:40:05", level: "error", source: "mcp", message: "calendar transport unavailable; retry scheduled" },
];

const INITIAL_SETTINGS = {
  language: "zh-CN",
  launchAtLogin: true,
  theme: "system",
  glass: "liquid",
  colorStyle: "dynamic",
  gradientPreset: "twilight",
  accent: "#6d5ce8",
  appIcon: "blue",
  backgroundMode: "wallpaper",
  wallpaper: WALLPAPERS.valley,
  fit: "cover",
  shade: 32,
  blur: 0,
  motion: "smooth",
  iconWeight: "regular",
  answerLayout: "timeline",
  verbosity: "normal",
  showTools: true,
  showSkills: true,
  showMcp: false,
  showHooks: true,
  showMemory: true,
  showStatus: true,
  showTimestamps: false,
  terminalExecutionMode: "system",
  terminalFont: "MesloLGS NF",
  terminalFontSize: 13,
  terminalLineHeight: 1.25,
  terminalScrollback: 5000,
  terminalCursorStyle: "bar",
  terminalCursorBlink: true,
  contextEnabled: true,
  contextSoftRatio: 70,
  contextMediumRatio: 82,
  contextHardRatio: 92,
  contextToolResultsLimit: 40,
  contextProtectLastN: 6,
  contextKeepTailBubbles: 4,
  memoryAutoRefresh: true,
  memoryApproval: false,
  memoryBackgroundReview: true,
  browserHomePage: "https://example.com/",
  browserLoopback: true,
  browserDownloadsEnabled: true,
  viewport: "1280 × 800",
};

Object.assign(window, {
  SETTINGS_GROUPS,
  ALL_TABS,
  WALLPAPERS,
  PROVIDERS,
  TOOLS,
  MODELS,
  LOG_ROWS,
  INITIAL_SETTINGS,
});
