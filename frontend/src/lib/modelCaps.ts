/** 模型能力展示辅助。 */
import type { ModelCapabilities } from "../types";

/** 将上下文窗口 token 数格式化为短标签（如 128K、1M）。 */
export function formatContextWindow(
  tokens?: number | null,
): string | null {
  if (tokens == null || !Number.isFinite(tokens) || tokens <= 0) return null;
  if (tokens >= 1_000_000) {
    const m = tokens / 1_000_000;
    return `${Number.isInteger(m) ? m : m.toFixed(1)}M`;
  }
  if (tokens >= 1_000) {
    const k = tokens / 1_000;
    return `${Number.isInteger(k) ? k : k.toFixed(1)}K`;
  }
  return String(Math.round(tokens));
}

/** 根据模型 ID / 提供商启发式推断能力标签（API 通常不返回细粒度能力）。 */
export function inferModelCapabilities(
  modelId: string,
  kind?: string,
): ModelCapabilities {
  const m = modelId.toLowerCase();
  const k = (kind ?? "").toLowerCase();

  const isNonChat =
    /embed|tts|whisper|dall-e|dalle|image|moderation|realtime|transcribe|speech/.test(
      m,
    );

  const vision =
    !isNonChat &&
    (/gpt-4o|gpt-4\.1|gpt-4-turbo|gpt-4-vision|o[34]|claude-3|claude-4|gemini|vision|glm-4v|qwen-vl|llava|pixtral|nova/.test(
      m,
    ) ||
      k === "google");

  const web =
    !isNonChat &&
    (/browse|search|online|grounding|web/.test(m) || k === "google");

  const reasoning =
    !isNonChat &&
    /o1|o3|o4|r1|reason|thinking|opus|deepseek-r|qwq|glm-z1/.test(m);

  const tools = !isNonChat;

  return { vision, web, reasoning, tools };
}
