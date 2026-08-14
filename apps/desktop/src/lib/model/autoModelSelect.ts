/**
 * Auto 智能选模：按任务启发式在已配置可用模型中路由（本地策略，非 Cursor API）。
 */
import { invoke } from "@tauri-apps/api/core";
import type { ChatInteractionMode } from "../chat/chatMode.ts";
import { inferModelCapabilities } from "./modelCaps.ts";
import type {
  ModelCapabilities,
  ModelInfo,
  ProviderDto,
  ProviderModelsResult,
} from "../../types.ts";

/** 模型能力档位（由 id 启发式推断） */
export type ModelTier = "lite" | "flash" | "standard" | "pro" | "reasoning";

/** 任务类型 */
export type TaskKind = "simple" | "vision" | "coding" | "reasoning" | "long";

/** 可选模型候选项 */
export type AutoModelCandidate = {
  providerId: string;
  providerName: string;
  providerKind: string;
  backendId: string;
  modelId: string;
  capabilities?: ModelCapabilities;
};

export type AutoSelectInput = {
  text: string;
  hasImages: boolean;
  chatMode: ChatInteractionMode;
  maxMode: boolean;
  candidates: AutoModelCandidate[];
  /** 优先留在当前提供商 */
  preferProviderId?: string | null;
};

export type AutoSelectResult = {
  providerId: string;
  modelId: string;
  tier: ModelTier;
  task: TaskKind;
};

/** 各任务偏好的档位顺序（越靠前越好） */
const TIER_PREFERENCE: Record<TaskKind, ModelTier[]> = {
  simple: ["lite", "flash", "standard", "pro", "reasoning"],
  vision: ["flash", "standard", "pro", "lite", "reasoning"],
  coding: ["flash", "standard", "pro", "reasoning", "lite"],
  reasoning: ["reasoning", "pro", "standard", "flash", "lite"],
  long: ["pro", "reasoning", "standard", "flash", "lite"],
};

/** 排除非对话模型 */
export function isChatModelId(modelId: string): boolean {
  const m = modelId.toLowerCase();
  return !/embed|tts|whisper|dall-e|dalle|moderation|realtime|transcribe|speech|imagen|image-gen|coding-agent/.test(
    m,
  );
}

/** 由模型 id 推断档位 */
export function classifyModelTier(modelId: string): ModelTier {
  const m = modelId.toLowerCase();
  // reasoning 优先（避免 o3-mini 落入 lite）
  if (/o[1-4](?:-|$)|\br1\b|reason|thinking|qwq|glm-z1|deepseek-r/.test(m)) {
    return "reasoning";
  }
  // 注意：不可用裸 `mini`（会误伤 gemini）
  if (
    /flash-lite|[-_]lite\b|lite[-_]|nano|haiku|[-_]8b\b|gpt-4o-mini|[-.]mini\b/.test(
      m,
    )
  ) {
    return "lite";
  }
  if (
    !/flash/.test(m) &&
    /(^|[-_.])pro($|[-_.])|opus|ultra|sonnet-4|claude-4|gpt-4\.5|gpt-4-turbo|gpt-4(?!o)/.test(
      m,
    )
  ) {
    return "pro";
  }
  if (/flash|turbo|fast/.test(m)) return "flash";
  if (/gpt-4o|sonnet|gemini|claude|gpt-4\.1/.test(m)) return "standard";
  return "standard";
}

/** 由消息内容 / 附件 / 模式推断任务类型 */
export function classifyTask(input: {
  text: string;
  hasImages: boolean;
  chatMode: ChatInteractionMode;
  maxMode: boolean;
}): TaskKind {
  if (input.hasImages) return "vision";
  const text = input.text.trim();
  if (text.length > 6000) return "long";
  if (input.maxMode) return "reasoning";
  if (input.chatMode === "plan") return "reasoning";
  if (
    /分析|推理|证明|架构|设计方案|trade-?off|\bwhy\b|compare|评估|深度思考|原理/.test(
      text,
    )
  ) {
    return "reasoning";
  }
  if (
    input.chatMode === "agent" ||
    input.chatMode === "multitask" ||
    /代码|实现|bug|修复|函数|组件|refactor|typescript|rust|python|写一?个|fix|debug|报错|编译/.test(
      text,
    )
  ) {
    return "coding";
  }
  if (
    text.length < 48 &&
    !/[`{}\n]/.test(text) &&
    (input.chatMode === "ask" || text.length < 32)
  ) {
    return "simple";
  }
  return "coding";
}

function versionBoost(modelId: string): number {
  const nums = modelId.match(/\d+(?:\.\d+)?/g);
  if (!nums?.length) return 0;
  const v = Number.parseFloat(nums[0] ?? "0");
  return Number.isFinite(v) ? Math.min(25, v * 2) : 0;
}

function scoreCandidate(
  c: AutoModelCandidate,
  task: TaskKind,
  maxMode: boolean,
  preferProviderId?: string | null,
): number {
  const tier = classifyModelTier(c.modelId);
  const prefs = TIER_PREFERENCE[task];
  const idx = prefs.indexOf(tier);
  let s = (prefs.length - (idx < 0 ? prefs.length : idx)) * 100;

  const caps =
    c.capabilities ?? inferModelCapabilities(c.modelId, c.providerKind);
  if (task === "vision") {
    if (!caps.vision) s -= 1000;
    else s += 60;
  }
  if (task === "reasoning" && caps.reasoning) s += 40;
  if (maxMode && (tier === "pro" || tier === "reasoning")) s += 80;
  if (preferProviderId && c.providerId === preferProviderId) s += 35;
  s += versionBoost(c.modelId);
  if (/preview|exp|experimental|beta|tts/.test(c.modelId.toLowerCase())) {
    s -= 45;
  }
  return s;
}

/**
 * 在候选项中选出最适合当前任务的模型。
 * 无可用聊天模型时返回 null。
 */
export function selectAutoModel(
  input: AutoSelectInput,
): AutoSelectResult | null {
  const chatCandidates = input.candidates.filter((c) =>
    isChatModelId(c.modelId),
  );
  if (chatCandidates.length === 0) return null;

  const task = classifyTask(input);
  let best: AutoModelCandidate | null = null;
  let bestScore = -Infinity;

  for (const c of chatCandidates) {
    const s = scoreCandidate(
      c,
      task,
      input.maxMode,
      input.preferProviderId,
    );
    if (s > bestScore) {
      bestScore = s;
      best = c;
    }
  }
  if (!best) return null;
  return {
    providerId: best.providerId,
    modelId: best.modelId,
    tier: classifyModelTier(best.modelId),
    task,
  };
}

/** 确保默认模型出现在列表中 */
function ensureDefaultModel(models: ModelInfo[], defaultId: string): ModelInfo[] {
  const id = defaultId.trim();
  if (!id) return models;
  if (models.some((m) => m.id === id)) return models;
  return [
    {
      id,
      capabilities: inferModelCapabilities(id),
      meta_source: "default",
    },
    ...models,
  ];
}

/** 从各启用提供商缓存/默认模型收集 Auto 候选项 */
export async function loadModelCandidates(
  providers: ProviderDto[],
): Promise<AutoModelCandidate[]> {
  if (providers.length === 0) return [];
  const groups = await Promise.all(
    providers.map(async (p) => {
      let models: ModelInfo[] = [];
      try {
        const cached = await invoke<ProviderModelsResult | null>(
          "get_cached_provider_models",
          { id: p.id },
        );
        if (cached?.models?.length) models = cached.models;
      } catch {
        /* cache miss */
      }
      models = ensureDefaultModel(models, p.model);
      return models
        .filter((m) => isChatModelId(m.id))
        .map((m) => ({
          providerId: p.id,
          providerName: p.display_name,
          providerKind: p.kind,
          backendId: p.backend_id,
          modelId: m.id,
          capabilities:
            m.capabilities ?? inferModelCapabilities(m.id, p.kind),
        }));
    }),
  );
  return groups.flat();
}
