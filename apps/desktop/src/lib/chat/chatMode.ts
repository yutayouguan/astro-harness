/**
 * 聊天交互模式（Agent / Plan / Ask / MultiTask）。
 *
 * 模式行为说明由后端写入 system prompt（`InteractionMode::system_guidance`），
 * 前端只负责 UI 选择与 `interactionMode` 传参。
 */

export type ChatInteractionMode = "agent" | "plan" | "ask" | "multitask";

const STORAGE_KEY = "astro.chat.mode";

/** 模式切换授权条默认倒计时（秒） */
export const MODE_SWITCH_COUNTDOWN_SEC = 10;

/** 可选模式列表（UI 顺序） */
export const CHAT_MODES: ChatInteractionMode[] = [
  "agent",
  "plan",
  "ask",
  "multitask",
];

/** 从 localStorage 读取模式，缺省 `agent` */
export function loadChatMode(): ChatInteractionMode {
  try {
    const v = localStorage.getItem(STORAGE_KEY);
    if (v === "agent" || v === "plan" || v === "ask" || v === "multitask") {
      return v;
    }
  } catch {
    /* ignore */
  }
  return "agent";
}

/** 持久化聊天模式 */
export function saveChatMode(mode: ChatInteractionMode) {
  try {
    localStorage.setItem(STORAGE_KEY, mode);
  } catch {
    /* ignore */
  }
}

/** `switch_mode` 工具返回的结构化请求 */
export type ModeSwitchRequest = {
  to: "plan" | "agent";
  reason: string;
  summary?: string;
};

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

