/**
 * 思考强度偏好：与发送请求中的 thinking_enabled / reasoning_effort 映射。
 */

import type { ModelReasoningMeta, ReasoningEffort } from "../../types";

/** UI 思考级别（含 off + OpenRouter supported_efforts） */
export type ThinkingLevel =
  "off" | "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max";

/** 聊天思考偏好 */
export type ChatThinkingPrefs = {
  level: ThinkingLevel;
};

const EFFORT_ORDER: ThinkingLevel[] = [
  "none",
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
];

const DEFAULT_EFFORTS: ThinkingLevel[] = ["low", "high", "max"];

/** 是否为合法 ThinkingLevel（不含 off） */
export function parseEffortLevel(
  raw: string | null | undefined,
): ThinkingLevel | null {
  const s = (raw ?? "").trim().toLowerCase();
  if (
    s === "none" ||
    s === "minimal" ||
    s === "low" ||
    s === "medium" ||
    s === "high" ||
    s === "xhigh" ||
    s === "max"
  ) {
    return s;
  }
  return null;
}

/** 从 OpenRouter reasoning 元数据生成可选级别（mandatory 时不含 off） */
export function thinkingLevelsFromMeta(
  meta?: ModelReasoningMeta | null,
): ThinkingLevel[] {
  const mandatory = Boolean(meta?.mandatory);
  const raw = (meta?.supported_efforts ?? [])
    .map((e) => parseEffortLevel(e))
    .filter((e): e is ThinkingLevel => e != null && e !== "off");
  const efforts =
    raw.length > 0
      ? EFFORT_ORDER.filter((e) => raw.includes(e))
      : [...DEFAULT_EFFORTS];
  return mandatory ? efforts : (["off", ...efforts] as ThinkingLevel[]);
}

/** 模型切换时的默认级别 */
export function defaultThinkingLevelFromMeta(
  meta?: ModelReasoningMeta | null,
): ThinkingLevel {
  const levels = thinkingLevelsFromMeta(meta);
  if (meta?.mandatory) {
    return (
      parseEffortLevel(meta.default_effort) ??
      levels.find((l) => l !== "off") ??
      "high"
    );
  }
  if (meta?.default_enabled === false) {
    return levels.includes("off") ? "off" : (levels[0] ?? "off");
  }
  const preferred = parseEffortLevel(meta?.default_effort);
  if (preferred && levels.includes(preferred)) return preferred;
  if (levels.includes("high")) return "high";
  return levels.find((l) => l !== "off") ?? "high";
}

/** 映射为后端 API 字段 */
export function thinkingLevelToApi(level: ThinkingLevel): {
  enabled: boolean;
  effort: ReasoningEffort;
} {
  if (level === "off") {
    return { enabled: false, effort: "high" };
  }
  return { enabled: true, effort: level };
}

/** 是否启用思考（非 off） */
export function thinkingPrefsEnabled(prefs: ChatThinkingPrefs): boolean {
  return prefs.level !== "off";
}
