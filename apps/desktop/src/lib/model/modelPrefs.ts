/** 模型选择器全局与会话偏好。 */
import type { ModelReasoningMeta, ReasoningEffort } from "../../types";
import {
  defaultThinkingLevelFromMeta,
  parseEffortLevel,
  type ThinkingLevel,
} from "../chat/thinkingPrefs.ts";

export type ModelContextSize = "default" | "300k" | "1m";
/** 与后端 ReasoningEffort / OpenRouter supported_efforts 一致 */
export type ModelEffort =
  | "none"
  | "minimal"
  | "low"
  | "medium"
  | "high"
  | "xhigh"
  | "max"
  | "persistent";

export type ModelRuntimePrefs = {
  thinking: boolean;
  /** 预留：尚未接入 API */
  fast: boolean;
  /** 预留：尚未接入 API */
  context: ModelContextSize;
  effort: ModelEffort;
};

export type ModelPickerGlobals = {
  /** Auto：隐藏模型列表，发送时按启发式自动选模 */
  auto: boolean;
  maxMode: boolean;
};

const PREFS_KEY = "astro.model.runtimePrefs";
const GLOBALS_KEY = "astro.model.pickerGlobals";

export const DEFAULT_MODEL_PREFS: ModelRuntimePrefs = {
  thinking: true,
  fast: false,
  context: "default",
  effort: "high",
};

export const DEFAULT_PICKER_GLOBALS: ModelPickerGlobals = {
  auto: false,
  maxMode: false,
};

export function modelPrefsKey(providerId: string, modelId: string): string {
  return `${providerId}::${modelId}`;
}

export function loadAllModelPrefs(): Record<string, ModelRuntimePrefs> {
  try {
    const raw = localStorage.getItem(PREFS_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as Record<
      string,
      Partial<ModelRuntimePrefs>
    >;
    const out: Record<string, ModelRuntimePrefs> = {};
    for (const [k, v] of Object.entries(parsed)) {
      out[k] = normalizePrefs(v);
    }
    return out;
  } catch {
    return {};
  }
}

export function saveAllModelPrefs(map: Record<string, ModelRuntimePrefs>) {
  try {
    localStorage.setItem(PREFS_KEY, JSON.stringify(map));
  } catch {
    /* ignore */
  }
}

export function loadModelPrefs(
  providerId: string,
  modelId: string,
): ModelRuntimePrefs {
  const all = loadAllModelPrefs();
  return all[modelPrefsKey(providerId, modelId)] ?? { ...DEFAULT_MODEL_PREFS };
}

/** 是否已有用户保存过的该模型偏好 */
export function hasSavedModelPrefs(
  providerId: string,
  modelId: string,
): boolean {
  const all = loadAllModelPrefs();
  return Object.prototype.hasOwnProperty.call(
    all,
    modelPrefsKey(providerId, modelId),
  );
}

/** 用 OpenRouter reasoning 元数据生成默认偏好（仅无已保存偏好时） */
export function prefsFromReasoningMeta(
  meta?: ModelReasoningMeta | null,
): ModelRuntimePrefs {
  const level = defaultThinkingLevelFromMeta(meta);
  return {
    ...DEFAULT_MODEL_PREFS,
    ...thinkingLevelToModelPatch(level),
  };
}

export function upsertModelPrefs(
  providerId: string,
  modelId: string,
  patch: Partial<ModelRuntimePrefs>,
): ModelRuntimePrefs {
  const all = loadAllModelPrefs();
  const key = modelPrefsKey(providerId, modelId);
  const next = normalizePrefs({
    ...(all[key] ?? DEFAULT_MODEL_PREFS),
    ...patch,
  });
  all[key] = next;
  saveAllModelPrefs(all);
  return next;
}

export function loadPickerGlobals(): ModelPickerGlobals {
  const forced: ModelPickerGlobals = { auto: false, maxMode: false };
  try {
    const raw = localStorage.getItem(GLOBALS_KEY);
    if (raw) {
      const parsed = JSON.parse(raw) as Partial<ModelPickerGlobals>;
      if (parsed.auto || parsed.maxMode) {
        savePickerGlobals(forced);
      }
    }
  } catch {
    /* ignore */
  }
  return { ...forced };
}

export function savePickerGlobals(g: ModelPickerGlobals) {
  try {
    localStorage.setItem(GLOBALS_KEY, JSON.stringify(g));
  } catch {
    /* ignore */
  }
}

/** MAX Mode 已下线：仅规范化 globals，不再写回 maxMode。 */
export function syncMaxModeWithThinkingLevel(
  _level: ThinkingLevel,
): ModelPickerGlobals {
  return loadPickerGlobals();
}

function normalizeEffort(v: unknown): ModelEffort {
  const parsed = parseEffortLevel(typeof v === "string" ? v : undefined);
  if (parsed && parsed !== "off") return parsed;
  return "high";
}

function normalizePrefs(
  v: Partial<ModelRuntimePrefs> & { effort?: string },
): ModelRuntimePrefs {
  const context: ModelContextSize =
    v.context === "300k" || v.context === "1m" || v.context === "default"
      ? v.context
      : "default";
  return {
    thinking: v.thinking !== false,
    fast: Boolean(v.fast),
    context,
    effort: normalizeEffort(v.effort),
  };
}

/** 映射到请求参数（透传 OpenRouter / 厂商 effort 字符串） */
export function modelPrefsToApi(
  prefs: ModelRuntimePrefs,
  globals: ModelPickerGlobals = DEFAULT_PICKER_GLOBALS,
): { thinkingEnabled: boolean; reasoningEffort: ReasoningEffort } {
  if (globals.maxMode) {
    return { thinkingEnabled: true, reasoningEffort: "max" };
  }
  if (!prefs.thinking) {
    return { thinkingEnabled: false, reasoningEffort: "high" };
  }
  return {
    thinkingEnabled: true,
    reasoningEffort: prefs.effort,
  };
}

/** 同步到输入栏思考 pill */
export function modelPrefsToThinkingLevel(
  prefs: ModelRuntimePrefs,
  globals: ModelPickerGlobals = DEFAULT_PICKER_GLOBALS,
): ThinkingLevel {
  if (globals.maxMode) return "max";
  if (!prefs.thinking) return "off";
  return prefs.effort;
}

/** 输入栏思考级别写回当前模型配置 */
export function thinkingLevelToModelPatch(
  level: ThinkingLevel,
): Partial<ModelRuntimePrefs> {
  if (level === "off") {
    return { thinking: false };
  }
  return { thinking: true, effort: level };
}

/** 模型是否具备可配置的推理能力（能力位或 OpenRouter reasoning 元数据）。 */
export function modelSupportsReasoning(
  capsReasoning?: boolean | null,
  meta?: ModelReasoningMeta | null,
): boolean {
  if (capsReasoning) return true;
  if (!meta) return false;
  if (meta.mandatory) return true;
  if (meta.default_enabled === true) return true;
  if (meta.persistent_instructions?.trim()) return true;
  return (meta.supported_efforts ?? []).some((e) =>
    Boolean(parseEffortLevel(e)),
  );
}

/**
 * 编辑面板 Effort 选项：优先 OpenRouter `supported_efforts`；
 * 仅当明确支持推理但无档位列表时回退 low/high/max。
 */
export function effortChoicesFromMeta(
  meta?: ModelReasoningMeta | null,
  capsReasoning?: boolean | null,
): ModelEffort[] {
  if (!modelSupportsReasoning(capsReasoning, meta)) return [];
  const raw = (meta?.supported_efforts ?? [])
    .map((e) => parseEffortLevel(e))
    .filter(
      (e): e is ThinkingLevel =>
        e != null &&
        e !== "off" &&
        (e !== "persistent" || Boolean(meta?.persistent_instructions?.trim())),
    );
  if (raw.length > 0) {
    const efforts = EFFORT_ORDER_FOR_PICKER.filter((e) => raw.includes(e));
    if (meta?.persistent_instructions?.trim() && !efforts.includes("persistent")) {
      efforts.push("persistent");
    }
    return efforts;
  }
  const efforts: ModelEffort[] = ["low", "high", "max"];
  if (meta?.persistent_instructions?.trim()) efforts.push("persistent");
  return efforts;
}

const EFFORT_ORDER_FOR_PICKER: ModelEffort[] = [
  "none",
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
  "persistent",
];

/**
 * 按模型 context_window 生成可选上下文档位（不可超过窗口）。
 * 无窗口或不足 300K 时不展示可选项。
 */
export function contextChoicesForWindow(
  tokens?: number | null,
): ModelContextSize[] {
  if (tokens == null || !Number.isFinite(tokens) || tokens <= 0) return [];
  const out: ModelContextSize[] = [];
  if (tokens >= 300_000) out.push("300k");
  if (tokens >= 1_000_000) out.push("1m");
  return out;
}

/** 将已存偏好钳到模型真实可选项。 */
export function clampPrefsToModelConfig(
  prefs: ModelRuntimePrefs,
  opts: {
    capsReasoning?: boolean | null;
    reasoning?: ModelReasoningMeta | null;
    contextWindow?: number | null;
  },
): ModelRuntimePrefs {
  const efforts = effortChoicesFromMeta(opts.reasoning, opts.capsReasoning);
  const contexts = contextChoicesForWindow(opts.contextWindow);
  let thinking = prefs.thinking;
  let effort = prefs.effort;

  if (opts.reasoning?.mandatory) {
    thinking = true;
  }
  if (efforts.length === 0) {
    thinking = false;
  } else if (thinking && !efforts.includes(effort)) {
    const preferred = parseEffortLevel(opts.reasoning?.default_effort);
    if (preferred && preferred !== "off" && efforts.includes(preferred)) {
      effort = preferred;
    } else if (efforts.includes("high")) {
      effort = "high";
    } else {
      effort = efforts[0]!;
    }
  }

  let context = prefs.context;
  if (context !== "default" && !contexts.includes(context)) {
    context = "default";
  }

  return {
    thinking,
    fast: false,
    context,
    effort,
  };
}
