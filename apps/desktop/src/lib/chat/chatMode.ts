/** 用户直接选择的工作模式。问答由 Agent 自动判断，不再单列 Ask 模式。 */
export type ChatWorkMode = "agent" | "plan";

/** Agent 忙碌时用户发送消息的投递模式。 */
export type ChatSendMode = "queue" | "steer" | "interrupt";

import { readPreference, writePreference } from "../storage/preferenceStore.ts";

const SEND_MODE_KEY = "astro.chat.sendMode";

export const CHAT_SEND_MODES: ChatSendMode[] = ["queue", "steer", "interrupt"];

export function loadChatSendMode(): ChatSendMode {
  return readPreference(SEND_MODE_KEY, "steer", (value) =>
    value === "queue" || value === "steer" || value === "interrupt"
      ? value
      : "steer",
  );
}

export function saveChatSendMode(mode: ChatSendMode) {
  writePreference(SEND_MODE_KEY, mode);
}

/** 发给后端的交互模式；独立任务调度与工作模式正交。 */
export type ChatInteractionMode = ChatWorkMode;

const STORAGE_KEY = "astro.chat.mode";

/** 可选模式列表（UI 顺序） */
export const CHAT_MODES: ChatWorkMode[] = ["agent", "plan"];

/** 把历史 MultiTask 顶层模式迁移为 Agent 工作模式。 */
export function normalizeStoredChatMode(value: string | null): ChatWorkMode {
  if (value === "plan") return value;
  return "agent";
}

/** 从 localStorage 读取模式，缺省 `agent` */
export function loadChatMode(): ChatWorkMode {
  return readPreference(STORAGE_KEY, "agent", (value) =>
    normalizeStoredChatMode(value),
  );
}

/** 持久化聊天模式 */
export function saveChatMode(mode: ChatWorkMode) {
  writePreference(STORAGE_KEY, mode);
}

/** `switch_mode` 工具返回的结构化请求 */
export type ModeSwitchRequest = {
  to: "plan" | "agent";
  reason: string;
  summary?: string;
};

/** 收窄到 Plan 不需要打断用户；恢复执行能力必须显式审阅。 */
export function shouldAutoApproveModeSwitch(
  request: ModeSwitchRequest | null,
): boolean {
  return request?.to === "plan";
}

/** 解析工具结果中的 `astro_mode_switch` JSON；无效则 null */
export function parseModeSwitchResult(
  result: string | undefined | null,
): ModeSwitchRequest | null {
  if (!result?.trim()) return null;
  try {
    const j = JSON.parse(result) as {
      astro_mode_switch?: boolean;
      to?: string;
      reason?: string;
      summary?: string | null;
    };
    if (!j?.astro_mode_switch) return null;
    const to = String(j.to ?? "")
      .trim()
      .toLowerCase();
    if (to !== "plan" && to !== "agent") return null;
    const reason = String(j.reason ?? "").trim();
    if (!reason) return null;
    const summary =
      typeof j.summary === "string" && j.summary.trim()
        ? j.summary.trim()
        : undefined;
    return { to, reason, summary };
  } catch {
    return null;
  }
}
