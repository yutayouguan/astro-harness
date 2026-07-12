/**
 * 聊天交互模式（Agent / Plan / Ask / MultiTask）及发送前附加提示。
 */

export type ChatInteractionMode = "agent" | "plan" | "ask" | "multitask";

const STORAGE_KEY = "astro.chat.mode";

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

/** 发送前附加的模式提示（空则不加） */
export function chatModeHint(mode: ChatInteractionMode): string {
  switch (mode) {
    case "plan":
      return "\n\n---\n[Mode: Plan] 请先给出清晰分步计划，确认后再执行关键步骤。";
    case "ask":
      return "\n\n---\n[Mode: Ask] 仅回答与解释，除非用户明确要求，否则不要修改文件或主动调用工具。";
    case "multitask":
      return "\n\n---\n[Mode: MultiTask] 将目标拆成可并行子任务，协调完成并汇总结果。";
    case "agent":
    default:
      return "";
  }
}
