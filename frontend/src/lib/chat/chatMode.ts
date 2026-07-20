/**
 * 聊天交互模式（Agent / Plan / Ask / MultiTask）及发送前附加提示。
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

/** `request_mode_switch` 工具返回的结构化请求 */
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

/** 发送前附加的模式提示（空则不加） */
export function chatModeHint(mode: ChatInteractionMode): string {
  switch (mode) {
    case "plan":
      return (
        "\n\n---\n[Mode: Plan] 只读规划：可用 file_ops(read/list/search)、web_search、task_plan 等。" +
        "禁止写文件、terminal、code_exec、delegate。" +
        "计划就绪后调用 request_mode_switch(to=\"agent\", reason=…, summary=计划摘要) 请求执行授权。"
      );
    case "ask":
      return (
        "\n\n---\n[Mode: Ask] 只读问答：解释与检索为主，不要修改文件或执行有副作用的操作。" +
        "若需落地实现，可 request_mode_switch(to=\"agent\", …)。"
      );
    case "multitask":
      return "\n\n---\n[Mode: MultiTask] 将目标拆成可并行子任务，协调完成并汇总结果。";
    case "agent":
      return (
        "\n\n---\n[Mode: Agent] 可执行工具。复杂多步任务可先 request_mode_switch(to=\"plan\", reason=…) 进入规划。"
      );
    default:
      return "";
  }
}
