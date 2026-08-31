import type { NodeType } from "./loopTypes";

interface ValidationRule {
  field: string;
  label: string;
}

const REQUIRED_FIELDS: Partial<Record<NodeType, ValidationRule[]>> = {
  ai_agent_task: [{ field: "prompt_template", label: "指令" }],
  parameter_extraction: [{ field: "prompt_template", label: "提示词" }],
  question_classification: [{ field: "classes", label: "分类列表" }],
  image_generation: [{ field: "prompt_template", label: "提示词" }],
  video_generation: [{ field: "prompt_template", label: "提示词" }],
  music_generation: [{ field: "prompt_template", label: "提示词" }],
  text_to_speech: [{ field: "text_template", label: "朗读文本" }],
  voice_clone: [{ field: "reference_audio", label: "参考音频" }],
  speech_to_text: [{ field: "input_path", label: "音频路径" }],
  image_edit: [{ field: "input_image", label: "输入图片" }],
  translation: [
    { field: "text_template", label: "输入文本" },
    { field: "target_lang", label: "目标语言" },
  ],
  knowledge_retrieval: [{ field: "query_template", label: "查询文本" }],
  summarization: [{ field: "text_template", label: "输入文本" }],
  sentiment_analysis: [{ field: "text_template", label: "输入文本" }],
  vision_understanding: [{ field: "input_path", label: "图片路径" }],
  http_request: [{ field: "url_template", label: "URL" }],
  conditional: [{ field: "conditions", label: "条件表达式" }],
  scheduled_trigger: [{ field: "schedule", label: "Cron 表达式" }],
  webhook_trigger: [{ field: "path", label: "路径" }],
  email_trigger: [{ field: "host", label: "服务器地址" }],
  file_watch_trigger: [{ field: "watch_path", label: "监控目录" }],
  file_io: [{ field: "path", label: "路径" }],
  run_loop: [{ field: "workflow_id", label: "工作流 ID" }],
};

export function validateNodeConfig(
  nodeType: NodeType,
  config: Record<string, unknown>,
): string[] {
  const rules = REQUIRED_FIELDS[nodeType];
  if (!rules) return [];
  return rules
    .filter((r) => {
      const val = config[r.field];
      if (val === undefined || val === null || val === "") return true;
      if (Array.isArray(val) && val.length === 0) return true;
      return false;
    })
    .map((r) => r.label);
}
