/**
 * 思考强度偏好：与发送请求中的 thinking_enabled / reasoning_effort 映射。
 */

import type { ReasoningEffort } from "../../types";

/** UI 四档思考级别 */
export type ThinkingLevel = "off" | "low" | "high" | "max";

/** 聊天思考偏好 */
export type ChatThinkingPrefs = {
  level: ThinkingLevel;
};

/** 映射为后端 API 字段 */
export function thinkingLevelToApi(level: ThinkingLevel): {
  enabled: boolean;
  effort: ReasoningEffort;
} {
  switch (level) {
    case "off":
      return { enabled: false, effort: "high" };
    case "max":
      return { enabled: true, effort: "max" };
    case "low":
    case "high":
    default:
      return { enabled: true, effort: "high" };
  }
}

/** 是否启用思考（非 off） */
export function thinkingPrefsEnabled(prefs: ChatThinkingPrefs): boolean {
  return prefs.level !== "off";
}
