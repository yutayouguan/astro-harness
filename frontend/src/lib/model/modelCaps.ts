/** 模型能力展示辅助。 */
import type { ModelCapabilities, ModelPricingMeta } from "../../types";

/** 能力位展示顺序（与 ModelPicker 图标一致）。 */
export type ModelCapKey =
  | "tools"
  | "reasoning"
  | "vision"
  | "file"
  | "audio_in"
  | "web"
  | "image_gen"
  | "video_gen"
  | "audio_gen"
  | "music_gen";

export const MODEL_CAP_ORDER: ModelCapKey[] = [
  "tools",
  "reasoning",
  "vision",
  "file",
  "audio_in",
  "web",
  "image_gen",
  "video_gen",
  "audio_gen",
  "music_gen",
];

export const EMPTY_MODEL_CAPABILITIES: ModelCapabilities = {
  vision: false,
  web: false,
  reasoning: false,
  tools: false,
  file: false,
  audio_in: false,
  image_gen: false,
  video_gen: false,
  audio_gen: false,
  music_gen: false,
};

/** 返回为 true 的能力键（固定顺序）。 */
export function listActiveModelCaps(
  caps: ModelCapabilities | null | undefined,
): ModelCapKey[] {
  if (!caps) return [];
  return MODEL_CAP_ORDER.filter((k) => Boolean(caps[k]));
}

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

/** 格式化 OpenRouter 单价为 `$in/$out`（每百万 token）。 */
export function formatModelPrice(
  pricing?: ModelPricingMeta | null,
): string | null {
  const inp = pricing?.prompt_per_million;
  const out = pricing?.completion_per_million;
  if (
    (inp == null || !Number.isFinite(inp)) &&
    (out == null || !Number.isFinite(out))
  ) {
    return null;
  }
  const fmt = (n: number) => {
    if (n >= 10) return n.toFixed(2);
    if (n >= 1) return Number(n.toFixed(2)).toString();
    if (n >= 0.01) return Number(n.toFixed(3)).toString();
    return Number(n.toFixed(4)).toString();
  };
  const a = inp != null && Number.isFinite(inp) ? fmt(inp) : "—";
  const b = out != null && Number.isFinite(out) ? fmt(out) : "—";
  return `$${a}/$${b}`;
}

/** 知识截止日期短标签（取 YYYY-MM 或原串）。 */
export function formatKnowledgeCutoff(
  cutoff?: string | null,
): string | null {
  const s = cutoff?.trim();
  if (!s) return null;
  const m = s.match(/^(\d{4}-\d{2})/);
  return m?.[1] ?? s;
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
    (/o1|o3|o4|r1|reason|thinking|opus|deepseek-r|deepseek-v4|qwq|glm-z1/.test(
      m,
    ) ||
      (k === "deepseek" && /v4|reasoner|r1/.test(m)));

  const tools = !isNonChat;

  return {
    vision,
    web,
    reasoning,
    tools,
    file: false,
    audio_in: false,
    image_gen: false,
    video_gen: false,
    audio_gen: false,
    music_gen: false,
  };
}
