import type { NodeType } from "./loopTypes";

export interface TemplateNode {
  id: string;
  node_type: NodeType;
  label: string;
  config: Record<string, unknown>;
}

export interface TemplateEdge {
  source: string;
  target: string;
  source_handle?: string | null;
}

export interface LoopTemplate {
  id: string;
  name: string;
  nameEn: string;
  description: string;
  descriptionEn: string;
  icon: string;
  nodes: TemplateNode[];
  edges: TemplateEdge[];
}

export const LOOP_TEMPLATES: LoopTemplate[] = [
  {
    id: "customer_service",
    name: "智能客服",
    nameEn: "Customer Service Bot",
    description: "问题分类 → 分支处理 → 合并回答",
    descriptionEn: "Classify question → branch handling → merge response",
    icon: "Headset",
    nodes: [
      { id: "t1", node_type: "manual_trigger", label: "用户提问", config: {} },
      { id: "n1", node_type: "question_classification", label: "问题分类", config: { classes: [{ id: "tech", label: "技术问题" }, { id: "billing", label: "账单问题" }, { id: "general", label: "通用问题" }] } },
      { id: "n2", node_type: "ai_agent_task", label: "技术处理", config: { prompt_template: "你是技术支持专家，请回答：{{trigger_input.question}}" } },
      { id: "n3", node_type: "ai_agent_task", label: "账单处理", config: { prompt_template: "你是账单专员，请处理：{{trigger_input.question}}" } },
      { id: "n4", node_type: "ai_agent_task", label: "通用处理", config: { prompt_template: "请回答用户问题：{{trigger_input.question}}" } },
      { id: "n5", node_type: "merge", label: "合并回答", config: {} },
      { id: "n6", node_type: "output", label: "输出回答", config: {} },
    ],
    edges: [
      { source: "t1", target: "n1" },
      { source: "n1", target: "n2", source_handle: "tech" },
      { source: "n1", target: "n3", source_handle: "billing" },
      { source: "n1", target: "n4", source_handle: "general" },
      { source: "n2", target: "n5" },
      { source: "n3", target: "n5" },
      { source: "n4", target: "n5" },
      { source: "n5", target: "n6" },
    ],
  },
  {
    id: "content_pipeline",
    name: "内容生产线",
    nameEn: "Content Pipeline",
    description: "AI 写文 → 翻译 → 配图 → 语音朗读",
    descriptionEn: "AI writing → translate → illustrate → TTS",
    icon: "Newspaper",
    nodes: [
      { id: "t1", node_type: "manual_trigger", label: "输入主题", config: {} },
      { id: "n1", node_type: "ai_agent_task", label: "AI 写文", config: { prompt_template: "请写一篇关于「{{trigger_input.topic}}」的文章，800字左右" } },
      { id: "n2", node_type: "translation", label: "翻译为英文", config: { source_lang: "zh", target_lang: "en" } },
      { id: "n3", node_type: "image_generation", label: "生成配图", config: { prompt_template: "为以下文章生成一张插图：{{n1.response}}" } },
      { id: "n4", node_type: "text_to_speech", label: "语音朗读", config: { text_template: "{{n1.response}}" } },
      { id: "n5", node_type: "output", label: "输出结果", config: {} },
    ],
    edges: [
      { source: "t1", target: "n1" },
      { source: "n1", target: "n2" },
      { source: "n1", target: "n3" },
      { source: "n1", target: "n4" },
      { source: "n2", target: "n5" },
      { source: "n3", target: "n5" },
      { source: "n4", target: "n5" },
    ],
  },
  {
    id: "data_etl",
    name: "数据处理",
    nameEn: "Data ETL",
    description: "HTTP 抓取 → JSON 解析 → 过滤 → 排序 → 输出",
    descriptionEn: "HTTP fetch → parse → filter → sort → output",
    icon: "Database",
    nodes: [
      { id: "t1", node_type: "manual_trigger", label: "触发", config: {} },
      { id: "n1", node_type: "http_request", label: "抓取数据", config: { method: "GET", url_template: "{{trigger_input.url}}" } },
      { id: "n2", node_type: "json", label: "解析 JSON", config: {} },
      { id: "n3", node_type: "filter", label: "过滤数据", config: {} },
      { id: "n4", node_type: "sort", label: "排序", config: {} },
      { id: "n5", node_type: "output", label: "输出结果", config: {} },
    ],
    edges: [
      { source: "t1", target: "n1" },
      { source: "n1", target: "n2" },
      { source: "n2", target: "n3" },
      { source: "n3", target: "n4" },
      { source: "n4", target: "n5" },
    ],
  },
  {
    id: "multimedia_gen",
    name: "多媒体批量生成",
    nameEn: "Multimedia Generation",
    description: "AI 文案 → 同步生成图片+视频+音乐",
    descriptionEn: "AI copy → parallel image + video + music generation",
    icon: "Clapperboard",
    nodes: [
      { id: "t1", node_type: "manual_trigger", label: "输入主题", config: {} },
      { id: "n1", node_type: "ai_agent_task", label: "生成文案", config: { prompt_template: "为「{{trigger_input.topic}}」写一段宣传文案" } },
      { id: "n2", node_type: "image_generation", label: "生成海报", config: { prompt_template: "{{n1.response}} — 宣传海报风格" } },
      { id: "n3", node_type: "video_generation", label: "生成视频", config: { prompt_template: "{{n1.response}}" } },
      { id: "n4", node_type: "music_generation", label: "生成 BGM", config: { prompt_template: "适合「{{trigger_input.topic}}」的背景音乐" } },
      { id: "n5", node_type: "output", label: "打包输出", config: {} },
    ],
    edges: [
      { source: "t1", target: "n1" },
      { source: "n1", target: "n2" },
      { source: "n1", target: "n3" },
      { source: "n1", target: "n4" },
      { source: "n2", target: "n5" },
      { source: "n3", target: "n5" },
      { source: "n4", target: "n5" },
    ],
  },
  {
    id: "document_analysis",
    name: "文档分析助手",
    nameEn: "Document Analysis",
    description: "图片理解 → 摘要 → 情感分析 → 通知",
    descriptionEn: "Vision → summarize → sentiment → notify",
    icon: "FileSearch",
    nodes: [
      { id: "t1", node_type: "manual_trigger", label: "上传文档", config: {} },
      { id: "n1", node_type: "vision_understanding", label: "图片理解", config: { prompt_template: "提取这张图片中的所有文字" } },
      { id: "n2", node_type: "summarization", label: "文本摘要", config: { text_template: "{{n1.description}}", style: "bullet" } },
      { id: "n3", node_type: "sentiment_analysis", label: "情感分析", config: { text_template: "{{n1.description}}" } },
      { id: "n4", node_type: "send_notification", label: "通知结果", config: { channel: "system", title_template: "文档分析完成", body_template: "摘要：{{n2.summary}}\n情感：{{n3.label}}" } },
    ],
    edges: [
      { source: "t1", target: "n1" },
      { source: "n1", target: "n2" },
      { source: "n1", target: "n3" },
      { source: "n2", target: "n4" },
      { source: "n3", target: "n4" },
    ],
  },
];
