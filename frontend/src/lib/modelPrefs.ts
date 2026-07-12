/** 模型选择器全局与会话偏好。 */
import type { ReasoningEffort } from "../types";
import type { ThinkingLevel } from "./thinkingPrefs";

export type ModelContextSize = "default" | "300k" | "1m";
/** 与后端 ReasoningEffort / DeepSeek 请求一致 */
export type ModelEffort = "low" | "medium" | "high" | "xhigh" | "max";

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
    const parsed = JSON.parse(raw) as Record<string, Partial<ModelRuntimePrefs>>;
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

export function upsertModelPrefs(
  providerId: string,
  modelId: string,
  patch: Partial<ModelRuntimePrefs>,
): ModelRuntimePrefs {
  const all = loadAllModelPrefs();
  const key = modelPrefsKey(providerId, modelId);
  const next = normalizePrefs({ ...(all[key] ?? DEFAULT_MODEL_PREFS), ...patch });
  all[key] = next;
  saveAllModelPrefs(all);
  return next;
}

export function loadPickerGlobals(): ModelPickerGlobals {
  try {
    const raw = localStorage.getItem(GLOBALS_KEY);
    if (!raw) return { ...DEFAULT_PICKER_GLOBALS };
    const parsed = JSON.parse(raw) as Partial<ModelPickerGlobals>;
    return {
      auto: Boolean(parsed.auto),
      maxMode: Boolean(parsed.maxMode),
    };
  } catch {
    return { ...DEFAULT_PICKER_GLOBALS };
  }
}

export function savePickerGlobals(g: ModelPickerGlobals) {
  try {
    localStorage.setItem(GLOBALS_KEY, JSON.stringify(g));
  } catch {
    /* ignore */
  }
}

/** 输入栏改思考级别时，与 MAX Mode 对齐，避免 UI Off 仍强制 max 发送 */
export function syncMaxModeWithThinkingLevel(level: ThinkingLevel): ModelPickerGlobals {
  const g = loadPickerGlobals();
  const next =
    level === "max"
      ? { ...g, maxMode: true }
      : g.maxMode
        ? { ...g, maxMode: false }
        : g;
  if (next.maxMode !== g.maxMode) savePickerGlobals(next);
  return next;
}

function normalizeEffort(v: unknown): ModelEffort {
  if (v === "low" || v === "medium" || v === "high" || v === "xhigh" || v === "max") {
    return v;
  }
  return "high";
}

function normalizePrefs(v: Partial<ModelRuntimePrefs> & { effort?: string }): ModelRuntimePrefs {
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

/** 映射到当前 DeepSeek / OpenAI 兼容请求参数 */
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
    reasoningEffort: prefs.effort === "max" ? "max" : "high",
  };
}

/** 同步到输入栏思考 pill */
export function modelPrefsToThinkingLevel(
  prefs: ModelRuntimePrefs,
  globals: ModelPickerGlobals = DEFAULT_PICKER_GLOBALS,
): ThinkingLevel {
  if (globals.maxMode) return "max";
  if (!prefs.thinking) return "off";
  if (prefs.effort === "max") return "max";
  return "high";
}

/** 输入栏思考级别写回当前模型配置（low 与 high 同映射到 API high） */
export function thinkingLevelToModelPatch(
  level: ThinkingLevel,
): Partial<ModelRuntimePrefs> {
  switch (level) {
    case "off":
      return { thinking: false };
    case "max":
      return { thinking: true, effort: "max" };
    case "low":
    case "high":
    default:
      return { thinking: true, effort: "high" };
  }
}
