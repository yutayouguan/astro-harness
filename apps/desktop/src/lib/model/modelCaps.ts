/** 模型能力展示辅助。 */
import type {
  ChatAttachmentKind,
  ModelCapabilities,
  ModelPricingMeta,
} from "../../types";

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

/** Unix 秒 → YYYY-MM-DD（本地日历日）。 */
export function formatModelCreated(created?: number | null): string | null {
  if (created == null || !Number.isFinite(created) || created <= 0) return null;
  const d = new Date(created * 1000);
  if (Number.isNaN(d.getTime())) return null;
  const y = d.getFullYear();
  const mo = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${mo}-${day}`;
}

const MS_PER_DAY = 24 * 60 * 60 * 1000;

/** `created`（Unix 秒）是否落在最近 `days` 天内（含未来极小误差）。 */
export function isModelCreatedWithinDays(
  created: number | null | undefined,
  days: number,
  nowMs: number = Date.now(),
): boolean {
  if (
    created == null ||
    !Number.isFinite(created) ||
    created <= 0 ||
    !Number.isFinite(days) ||
    days <= 0
  ) {
    return false;
  }
  const ageMs = nowMs - created * 1000;
  return ageMs >= -MS_PER_DAY && ageMs <= days * MS_PER_DAY;
}

/** 按 `created` 降序；无时间戳的相对顺序保持稳定（返回 0），同秒再比 id。 */
export function compareModelsByCreatedDesc(
  a: { id?: string; created?: number | null },
  b: { id?: string; created?: number | null },
): number {
  const ac =
    a.created != null && Number.isFinite(a.created) && a.created > 0
      ? a.created
      : 0;
  const bc =
    b.created != null && Number.isFinite(b.created) && b.created > 0
      ? b.created
      : 0;
  if (ac === 0 && bc === 0) return 0;
  if (ac === 0) return 1;
  if (bc === 0) return -1;
  if (ac !== bc) return bc - ac;
  return (a.id ?? "").localeCompare(b.id ?? "");
}

const FILE_ACCEPT =
  ".pdf,.txt,.md,.json,.csv,.doc,.docx,.xls,.xlsx,.ppt,.pptx,.zip,.rs,.ts,.tsx,.js,.py";

/**
 * 按模型输入能力生成 `<input accept>`。
 * 能力未知（null）时放行全部常见类型，避免误伤。
 */
export function attachmentAcceptForCaps(
  caps: ModelCapabilities | null | undefined,
): string {
  if (!caps) {
    return `image/*,video/*,audio/*,${FILE_ACCEPT}`;
  }
  const parts: string[] = [];
  if (caps.vision) parts.push("image/*", "video/*");
  if (caps.audio_in) parts.push("audio/*");
  if (caps.file) parts.push(FILE_ACCEPT);
  return parts.length > 0 ? parts.join(",") : "";
}

/** 某附件 kind 是否被当前模型能力允许。 */
export function attachmentKindAllowed(
  kind: ChatAttachmentKind,
  caps: ModelCapabilities | null | undefined,
): boolean {
  if (!caps) return true;
  switch (kind) {
    case "image":
    case "video":
      return Boolean(caps.vision);
    case "audio":
      return Boolean(caps.audio_in);
    case "file":
      return Boolean(caps.file);
    case "folder":
      return true;
    default:
      return false;
  }
}

/**
 * 粗估单轮费用（USD）：输入按字数/4，输出按预设 completion tokens。
 * 缺单价时返回 null。
 */
export function estimateTurnCostUsd(opts: {
  pricing?: ModelPricingMeta | null;
  inputChars: number;
  /** 缺省用 1024 */
  expectedOutputTokens?: number | null;
}): number | null {
  const inp = opts.pricing?.prompt_per_million;
  const out = opts.pricing?.completion_per_million;
  if (
    (inp == null || !Number.isFinite(inp)) &&
    (out == null || !Number.isFinite(out))
  ) {
    return null;
  }
  const inTokens = Math.max(0, Math.ceil(opts.inputChars / 4));
  const outTokens = Math.max(
    0,
    Math.round(
      opts.expectedOutputTokens != null &&
        Number.isFinite(opts.expectedOutputTokens) &&
        opts.expectedOutputTokens > 0
        ? opts.expectedOutputTokens
        : 1024,
    ),
  );
  const a = inp != null && Number.isFinite(inp) ? inp : 0;
  const b = out != null && Number.isFinite(out) ? out : 0;
  return (inTokens * a + outTokens * b) / 1_000_000;
}

/** 格式化估费为短标签（如 `~$0.012`）。 */
export function formatEstimateCostUsd(usd: number | null | undefined): string | null {
  if (usd == null || !Number.isFinite(usd) || usd < 0) return null;
  if (usd === 0) return "~$0";
  if (usd < 0.0001) return "<$0.0001";
  const fmt = (n: number, digits: number) =>
    Number(n.toFixed(digits)).toString();
  if (usd < 0.01) return `~$${fmt(usd, 4)}`;
  if (usd < 1) return `~$${fmt(usd, 3)}`;
  return `~$${fmt(usd, 2)}`;
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
