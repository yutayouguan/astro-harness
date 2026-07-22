/** Loop 工作流 TypeScript 类型（对齐 Rust workflow crate model.rs） */

import type { LucidePaint, LucideRenderStyle } from "../../lib/agent/lucideAgentIcons";

/** 解析后的 Loop 图标数据 */
export interface LoopIconData {
  id: string;
  paint?: LucidePaint;
  style?: LucideRenderStyle;
}

export function parseLoopIcon(raw?: string | null): LoopIconData | null {
  if (!raw) return null;
  try {
    const obj = JSON.parse(raw);
    if (typeof obj.id === "string") return obj as LoopIconData;
  } catch { /* ignore */ }
  return null;
}

export function serializeLoopIcon(data: LoopIconData): string {
  return JSON.stringify(data);
}

export type NodeType =
  // 触发器
  | "manual_trigger"
  | "scheduled_trigger"
  | "webhook_trigger"
  // AI
  | "ai_agent_task"
  | "parameter_extraction"
  | "question_classification"
  // 多媒体生成
  | "image_generation"
  | "video_generation"
  | "music_generation"
  | "text_to_speech"
  | "subtitle_generation"
  // 流程控制
  | "conditional"
  | "multi_branch"
  | "filter"
  | "merge"
  | "loop"
  | "human_approval"
  // 数据处理
  | "set_fields"
  | "format_text"
  | "json"
  | "code"
  | "sort"
  | "slice"
  | "aggregate"
  // 动作
  | "http_request"
  | "run_loop"
  | "delay_wait"
  | "output"
  | "audio_processing"
  // 自定义（引用已保存的工作流）
  | "custom_loop";

export type NodeCategory =
  | "trigger"
  | "ai"
  | "media"
  | "flow_control"
  | "data_processing"
  | "action"
  | "custom";

export interface Position {
  x: number;
  y: number;
}

export interface LoopNodeDto {
  id: string;
  node_type: NodeType;
  label: string;
  position: Position;
  config: Record<string, unknown>;
  disabled: boolean;
}

export interface LoopEdgeDto {
  id: string;
  source: string;
  source_handle: string | null;
  target: string;
  target_handle: string | null;
}

export interface LoopDto {
  id: string;
  name: string;
  description: string;
  enabled: boolean;
  ai_callable: boolean;
  nodes: LoopNodeDto[];
  edges: LoopEdgeDto[];
  variables: Record<string, unknown>;
  created_at: string;
  updated_at: string;
  icon?: string;
}

export interface LoopRunDto {
  id: string;
  workflow_id: string;
  workflow_name: string;
  trigger_type: string;
  started_at: string;
  finished_at: string | null;
  status: string;
  error: string | null;
  node_count: number;
  output: string | null;
}

export interface LoopStepLogDto {
  id: string;
  run_id: string;
  node_id: string;
  node_type: string;
  node_label: string;
  started_at: string;
  finished_at: string | null;
  status: string;
  input: string | null;
  output: string | null;
  error: string | null;
}

export interface NodeMeta {
  type: NodeType;
  category: NodeCategory;
  label: string;
  labelEn: string;
  icon: string;
  color: string;
}

export const NODE_CATEGORIES: {
  key: NodeCategory;
  label: string;
  labelEn: string;
  icon: string;
  color: string;
}[] = [
  { key: "trigger", label: "触发器", labelEn: "Triggers", icon: "MousePointerClick", color: "#6366f1" },
  { key: "ai", label: "AI", labelEn: "AI", icon: "Sparkles", color: "#6366f1" },
  { key: "media", label: "多媒体生成", labelEn: "Media", icon: "Image", color: "#6366f1" },
  { key: "flow_control", label: "流程控制", labelEn: "Flow Control", icon: "Route", color: "#6366f1" },
  { key: "data_processing", label: "数据处理", labelEn: "Data Processing", icon: "Braces", color: "#6366f1" },
  { key: "action", label: "动作", labelEn: "Actions", icon: "Play", color: "#6366f1" },
  { key: "custom", label: "自定义", labelEn: "Custom", icon: "Puzzle", color: "#6366f1" },
];

export const NODE_REGISTRY: NodeMeta[] = [
  // 触发器
  { type: "manual_trigger", category: "trigger", label: "手动触发", labelEn: "Manual Trigger", icon: "Hand", color: "#60a5fa" },
  { type: "scheduled_trigger", category: "trigger", label: "定时触发", labelEn: "Scheduled Trigger", icon: "Clock", color: "#60a5fa" },
  { type: "webhook_trigger", category: "trigger", label: "Webhook 触发", labelEn: "Webhook Trigger", icon: "Webhook", color: "#60a5fa" },
  // AI
  { type: "ai_agent_task", category: "ai", label: "AI 智能体任务", labelEn: "AI Agent Task", icon: "Brain", color: "#a78bfa" },
  { type: "parameter_extraction", category: "ai", label: "参数提取", labelEn: "Parameter Extraction", icon: "FileSearch", color: "#a78bfa" },
  { type: "question_classification", category: "ai", label: "问题分类", labelEn: "Question Classification", icon: "Tag", color: "#a78bfa" },
  // 多媒体生成
  { type: "image_generation", category: "media", label: "生成图片", labelEn: "Image Generation", icon: "Image", color: "#f472b6" },
  { type: "video_generation", category: "media", label: "生成视频", labelEn: "Video Generation", icon: "Video", color: "#f472b6" },
  { type: "music_generation", category: "media", label: "生成音乐", labelEn: "Music Generation", icon: "Music", color: "#f472b6" },
  { type: "text_to_speech", category: "media", label: "文字转语音", labelEn: "Text to Speech", icon: "AudioLines", color: "#f472b6" },
  { type: "subtitle_generation", category: "media", label: "字幕生成", labelEn: "Subtitle Generation", icon: "Captions", color: "#f472b6" },
  // 流程控制
  { type: "conditional", category: "flow_control", label: "条件判断", labelEn: "Conditional", icon: "GitBranch", color: "#34d399" },
  { type: "multi_branch", category: "flow_control", label: "多路分支", labelEn: "Multi Branch", icon: "GitFork", color: "#34d399" },
  { type: "filter", category: "flow_control", label: "过滤", labelEn: "Filter", icon: "Filter", color: "#34d399" },
  { type: "merge", category: "flow_control", label: "合并", labelEn: "Merge", icon: "GitMerge", color: "#34d399" },
  { type: "loop", category: "flow_control", label: "循环", labelEn: "Loop", icon: "Repeat", color: "#34d399" },
  { type: "human_approval", category: "flow_control", label: "人工审批", labelEn: "Human Approval", icon: "UserCheck", color: "#34d399" },
  // 数据处理
  { type: "set_fields", category: "data_processing", label: "设置字段", labelEn: "Set Fields", icon: "Braces", color: "#fbbf24" },
  { type: "format_text", category: "data_processing", label: "格式化文本", labelEn: "Format Text", icon: "Type", color: "#fbbf24" },
  { type: "json", category: "data_processing", label: "JSON", labelEn: "JSON", icon: "FileJson", color: "#fbbf24" },
  { type: "code", category: "data_processing", label: "代码", labelEn: "Code", icon: "Code", color: "#fbbf24" },
  { type: "sort", category: "data_processing", label: "排序", labelEn: "Sort", icon: "ArrowUpDown", color: "#fbbf24" },
  { type: "slice", category: "data_processing", label: "截取", labelEn: "Slice", icon: "Scissors", color: "#fbbf24" },
  { type: "aggregate", category: "data_processing", label: "聚合", labelEn: "Aggregate", icon: "Sigma", color: "#fbbf24" },
  // 动作
  { type: "http_request", category: "action", label: "HTTP 请求", labelEn: "HTTP Request", icon: "Globe", color: "#fb923c" },
  { type: "run_loop", category: "action", label: "运行 Loop", labelEn: "Run Loop", icon: "Play", color: "#fb923c" },
  { type: "delay_wait", category: "action", label: "延时等待", labelEn: "Delay / Wait", icon: "Timer", color: "#fb923c" },
  { type: "output", category: "action", label: "输出", labelEn: "Output", icon: "ArrowRightFromLine", color: "#fb923c" },
  { type: "audio_processing", category: "action", label: "音频处理", labelEn: "Audio Processing", icon: "AudioWaveform", color: "#fb923c" },
  // 自定义
  { type: "custom_loop", category: "custom", label: "自定义 Loop", labelEn: "Custom Loop", icon: "Puzzle", color: "#8b5cf6" },
];

export function getNodeMeta(type: NodeType): NodeMeta {
  return NODE_REGISTRY.find((m) => m.type === type) ?? NODE_REGISTRY[0];
}

export function getNodesByCategory(category: NodeCategory): NodeMeta[] {
  return NODE_REGISTRY.filter((m) => m.category === category);
}
