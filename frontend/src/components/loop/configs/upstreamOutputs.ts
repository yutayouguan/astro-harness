/** 上游节点输出字段定义 — 用于变量选择器 */

import type { NodeType } from "../loopTypes";

export type MediaType = "image" | "video" | "audio" | "text" | "path" | "any";

export interface OutputField {
  key: string;
  label: string;
  mediaType: MediaType;
}

export interface UpstreamOutput {
  nodeId: string;
  nodeLabel: string;
  nodeType: NodeType;
  fields: OutputField[];
}

const OUTPUT_SCHEMAS: Partial<Record<NodeType, OutputField[]>> = {
  image_generation: [
    { key: "images", label: "图片列表", mediaType: "image" },
    { key: "count", label: "数量", mediaType: "any" },
  ],
  video_generation: [
    { key: "path", label: "视频文件", mediaType: "video" },
    { key: "mime_type", label: "MIME 类型", mediaType: "any" },
  ],
  music_generation: [
    { key: "path", label: "音乐文件", mediaType: "audio" },
    { key: "duration_ms", label: "时长(ms)", mediaType: "any" },
  ],
  text_to_speech: [
    { key: "path", label: "语音文件", mediaType: "audio" },
    { key: "duration_ms", label: "时长(ms)", mediaType: "any" },
  ],
  voice_clone: [
    { key: "path", label: "克隆语音", mediaType: "audio" },
  ],
  speech_to_text: [
    { key: "text", label: "识别文本", mediaType: "text" },
  ],
  image_edit: [
    { key: "path", label: "编辑后图片", mediaType: "image" },
  ],
  ai_agent_task: [
    { key: "text", label: "AI 输出", mediaType: "text" },
  ],
  summarization: [
    { key: "text", label: "摘要", mediaType: "text" },
  ],
  translation: [
    { key: "translation", label: "译文", mediaType: "text" },
  ],
  parameter_extraction: [
    { key: "result", label: "提取结果", mediaType: "any" },
  ],
  question_classification: [
    { key: "category", label: "分类结果", mediaType: "text" },
  ],
  sentiment_analysis: [
    { key: "result", label: "情感分析", mediaType: "text" },
  ],
  document_understanding: [
    { key: "text", label: "文档内容", mediaType: "text" },
  ],
  http_request: [
    { key: "body", label: "响应体", mediaType: "any" },
    { key: "status", label: "状态码", mediaType: "any" },
  ],
  code: [
    { key: "result", label: "代码结果", mediaType: "any" },
  ],
  set_fields: [
    { key: "result", label: "字段值", mediaType: "any" },
  ],
  format_text: [
    { key: "text", label: "格式化文本", mediaType: "text" },
  ],
  json: [
    { key: "result", label: "JSON 结果", mediaType: "any" },
  ],
  file_io: [
    { key: "path", label: "文件路径", mediaType: "path" },
    { key: "content", label: "文件内容", mediaType: "text" },
  ],
  audio_processing: [
    { key: "path", label: "处理后音频", mediaType: "audio" },
  ],
  subtitle_generation: [
    { key: "path", label: "字幕文件", mediaType: "path" },
    { key: "text", label: "字幕文本", mediaType: "text" },
  ],
  knowledge_retrieval: [
    { key: "text", label: "检索内容", mediaType: "text" },
  ],
};

export function getOutputFields(nodeType: NodeType): OutputField[] {
  return OUTPUT_SCHEMAS[nodeType] ?? [{ key: "result", label: "输出", mediaType: "any" }];
}

/** 根据 edges 反向遍历找到所有上游节点 */
export function computeUpstreamOutputs(
  currentNodeId: string,
  allNodes: { id: string; nodeType: NodeType; label: string }[],
  edges: { source: string; target: string }[],
): UpstreamOutput[] {
  const visited = new Set<string>();
  const queue = [currentNodeId];
  while (queue.length > 0) {
    const cur = queue.pop()!;
    for (const e of edges) {
      if (e.target === cur && !visited.has(e.source)) {
        visited.add(e.source);
        queue.push(e.source);
      }
    }
  }
  return allNodes
    .filter((n) => visited.has(n.id))
    .map((n) => ({
      nodeId: n.id,
      nodeLabel: n.label,
      nodeType: n.nodeType,
      fields: getOutputFields(n.nodeType),
    }));
}

/** 生成变量引用模板（简短 label 版本） */
export function varRef(nodeLabel: string, fieldKey: string): string {
  return `{{${nodeLabel}.${fieldKey}}}`;
}
