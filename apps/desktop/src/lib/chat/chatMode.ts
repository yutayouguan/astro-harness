/** 用户直接选择的工作模式。 */
export type ChatWorkMode = "agent" | "plan" | "ask";

/**
 * 发给后端的交互模式。
 *
 * `multitask` 是 Agent + 并行任务开关组合出的运行态，不再作为顶层工作模式展示。
 */
export type ChatInteractionMode = ChatWorkMode | "multitask";

const STORAGE_KEY = "astro.chat.mode";
const PARALLEL_TASKS_STORAGE_KEY = "astro.chat.parallelTasks";

/** 模式切换授权条默认倒计时（秒） */
export const MODE_SWITCH_COUNTDOWN_SEC = 10;

/** 可选模式列表（UI 顺序） */
export const CHAT_MODES: ChatWorkMode[] = ["agent", "plan", "ask"];

/** 把历史 MultiTask 顶层模式迁移为 Agent 工作模式。 */
export function normalizeStoredChatMode(value: string | null): ChatWorkMode {
  if (value === "plan" || value === "ask") return value;
  return "agent";
}

/** 新开关未写入时，继承历史 `multitask` 配置。 */
export function resolveStoredParallelTasks(
  storedMode: string | null,
  storedParallel: string | null,
): boolean {
  if (storedParallel === "true") return true;
  if (storedParallel === "false") return false;
  return storedMode === "multitask";
}

/** 工作模式与并行开关组合成后端已有的交互模式。 */
export function resolveInteractionMode(
  workMode: ChatWorkMode,
  parallelTasksEnabled: boolean,
): ChatInteractionMode {
  return workMode === "agent" && parallelTasksEnabled ? "multitask" : workMode;
}

/** 从 localStorage 读取模式，缺省 `agent` */
export function loadChatMode(): ChatWorkMode {
  try {
    return normalizeStoredChatMode(localStorage.getItem(STORAGE_KEY));
  } catch {
    /* ignore */
  }
  return "agent";
}

/** 持久化聊天模式 */
export function saveChatMode(mode: ChatWorkMode) {
  try {
    localStorage.setItem(STORAGE_KEY, mode);
  } catch {
    /* ignore */
  }
}

/** 读取 Agent 并行任务开关，并兼容旧版 MultiTask 顶层模式。 */
export function loadParallelTasksEnabled(): boolean {
  try {
    return resolveStoredParallelTasks(
      localStorage.getItem(STORAGE_KEY),
      localStorage.getItem(PARALLEL_TASKS_STORAGE_KEY),
    );
  } catch {
    return false;
  }
}

/** 持久化 Agent 并行任务开关。 */
export function saveParallelTasksEnabled(enabled: boolean) {
  try {
    localStorage.setItem(PARALLEL_TASKS_STORAGE_KEY, String(enabled));
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
