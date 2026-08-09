/**
 * 节点配置校验 — 定义每种节点类型的必填字段，运行前检查。
 */

import type { NodeType } from "../loopTypes";

export interface ValidationError {
  field: string;
  label: string;
  message: string;
}

interface FieldRule {
  key: string;
  label: string;
}

const REQUIRED_FIELDS: Partial<Record<NodeType, FieldRule[]>> = {
  ai_agent_task: [
    { key: "prompt_template", label: "指令" },
  ],
  image_generation: [
    { key: "prompt_template", label: "生成提示词" },
  ],
  video_generation: [
    { key: "prompt_template", label: "生成提示词" },
  ],
  music_generation: [
    { key: "prompt_template", label: "音乐描述" },
  ],
  text_to_speech: [
    { key: "text_template", label: "朗读文本" },
  ],
  voice_clone: [
    { key: "text_template", label: "合成文本" },
  ],
  speech_to_text: [
    { key: "input_path", label: "音频/视频源" },
  ],
  subtitle_generation: [
    { key: "audio_source", label: "音频/视频来源" },
  ],
  image_edit: [
    { key: "input_image", label: "输入图片" },
  ],
  translation: [
    { key: "text_template", label: "输入文本" },
    { key: "target_lang", label: "目标语言" },
  ],
  summarization: [
    { key: "text_template", label: "输入文本" },
  ],
  sentiment_analysis: [
    { key: "text_template", label: "输入文本" },
  ],
  parameter_extraction: [
    { key: "prompt_template", label: "提取指令" },
  ],
  document_understanding: [
    { key: "input_path", label: "输入路径" },
  ],
  http_request: [
    { key: "url", label: "URL" },
  ],
  conditional: [
    { key: "conditions", label: "条件分支" },
  ],
  send_notification: [
    { key: "body_template", label: "通知内容" },
  ],
  code: [
    { key: "code", label: "代码" },
  ],
  format_text: [
    { key: "template", label: "文本模板" },
  ],
  knowledge_retrieval: [
    { key: "query_template", label: "查询文本" },
  ],
};

function isEmpty(value: unknown): boolean {
  if (value === undefined || value === null) return true;
  if (typeof value === "string") return value.trim() === "";
  if (Array.isArray(value)) return value.length === 0;
  return false;
}

export function validateNodeConfig(
  nodeType: NodeType,
  config: Record<string, unknown>,
): ValidationError[] {
  const rules = REQUIRED_FIELDS[nodeType];
  if (!rules) return [];

  return rules
    .filter((rule) => isEmpty(config[rule.key]))
    .map((rule) => ({
      field: rule.key,
      label: rule.label,
      message: `「${rule.label}」不能为空`,
    }));
}

export function hasValidationErrors(
  nodeType: NodeType,
  config: Record<string, unknown>,
): boolean {
  return validateNodeConfig(nodeType, config).length > 0;
}
