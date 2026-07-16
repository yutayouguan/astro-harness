import type { ChatActivity } from "../../types";

/** 解析活动卡 Input/Output（仅用显式 `input` / `output`；`detail` 仅作 output 兜底）。 */
export function resolveActivityIO(activity: ChatActivity): {
  input?: string;
  output?: string;
} {
  if (activity.input != null || activity.output != null) {
    return {
      input: activity.input || undefined,
      output: activity.output || undefined,
    };
  }
  if (activity.detail) {
    return { output: activity.detail };
  }
  return {};
}

/** 是否有可展开正文 */
export function activityHasBody(activity: ChatActivity): boolean {
  const { input, output } = resolveActivityIO(activity);
  return Boolean(input || output);
}
