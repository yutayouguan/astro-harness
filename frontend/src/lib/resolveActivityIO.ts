import type { ChatActivity } from "../types";

const LEGACY_SEP = "\n→\n";

/** 解析活动卡 Input/Output；优先显式字段，否则兼容旧 detail。 */
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
  const detail = activity.detail;
  if (!detail) return {};
  const i = detail.indexOf(LEGACY_SEP);
  if (i < 0) return { output: detail };
  const input = detail.slice(0, i);
  const output = detail.slice(i + LEGACY_SEP.length);
  return {
    input: input || undefined,
    output: output || undefined,
  };
}

/** 是否有可展开正文 */
export function activityHasBody(activity: ChatActivity): boolean {
  const { input, output } = resolveActivityIO(activity);
  return Boolean(input || output);
}
