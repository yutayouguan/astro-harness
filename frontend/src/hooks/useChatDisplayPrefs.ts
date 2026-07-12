/** 聊天展示偏好（详细度、开关）持久化。 */
import { useCallback, useEffect, useState } from "react";

export type ChatVerbosity = "compact" | "normal" | "detailed";

export type ChatDisplayPrefs = {
  /** 整体详细程度预设 */
  verbosity: ChatVerbosity;
  /** 显示工具调用 */
  showTools: boolean;
  /** 显示 Skills 调用 */
  showSkills: boolean;
  /** 显示 MCP 调用 */
  showMcp: boolean;
  /** 显示 Hook 事件 */
  showHooks: boolean;
  /** 显示记忆更新 */
  showMemory: boolean;
  /** 显示状态/阶段信息 */
  showStatus: boolean;
  /** 显示时间戳 */
  showTimestamps: boolean;
};

const STORAGE_KEY = "astro.chat.displayPrefs";

const PRESETS: Record<ChatVerbosity, Omit<ChatDisplayPrefs, "verbosity">> = {
  compact: {
    showTools: false,
    showSkills: false,
    showMcp: false,
    showHooks: false,
    showMemory: false,
    showStatus: false,
    showTimestamps: false,
  },
  normal: {
    showTools: true,
    showSkills: true,
    showMcp: false,
    showHooks: false,
    showMemory: true,
    showStatus: true,
    showTimestamps: false,
  },
  detailed: {
    showTools: true,
    showSkills: true,
    showMcp: true,
    showHooks: true,
    showMemory: true,
    showStatus: true,
    showTimestamps: true,
  },
};

function defaultPrefs(): ChatDisplayPrefs {
  return { verbosity: "normal", ...PRESETS.normal };
}

function readStored(): ChatDisplayPrefs {
  const base = defaultPrefs();
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return base;
    const parsed = JSON.parse(raw) as Partial<ChatDisplayPrefs>;
    const verbosity =
      parsed.verbosity === "compact" ||
      parsed.verbosity === "normal" ||
      parsed.verbosity === "detailed"
        ? parsed.verbosity
        : base.verbosity;
    return {
      verbosity,
      showTools: typeof parsed.showTools === "boolean" ? parsed.showTools : PRESETS[verbosity].showTools,
      showSkills:
        typeof parsed.showSkills === "boolean" ? parsed.showSkills : PRESETS[verbosity].showSkills,
      showMcp: typeof parsed.showMcp === "boolean" ? parsed.showMcp : PRESETS[verbosity].showMcp,
      showHooks:
        typeof parsed.showHooks === "boolean" ? parsed.showHooks : PRESETS[verbosity].showHooks,
      showMemory:
        typeof parsed.showMemory === "boolean" ? parsed.showMemory : PRESETS[verbosity].showMemory,
      showStatus:
        typeof parsed.showStatus === "boolean" ? parsed.showStatus : PRESETS[verbosity].showStatus,
      showTimestamps:
        typeof parsed.showTimestamps === "boolean"
          ? parsed.showTimestamps
          : PRESETS[verbosity].showTimestamps,
    };
  } catch {
    return base;
  }
}

export type ChatActivityKind = "tool" | "skill" | "mcp" | "hook" | "memory" | "status";

export function isActivityVisible(
  kind: ChatActivityKind,
  prefs: ChatDisplayPrefs,
): boolean {
  switch (kind) {
    case "tool":
      return prefs.showTools;
    case "skill":
      return prefs.showSkills;
    case "mcp":
      return prefs.showMcp;
    case "hook":
      return prefs.showHooks;
    case "memory":
      return prefs.showMemory;
    case "status":
      return prefs.showStatus;
    default:
      return false;
  }
}

export function useChatDisplayPrefs() {
  const [prefs, setPrefs] = useState<ChatDisplayPrefs>(() =>
    typeof window === "undefined" ? defaultPrefs() : readStored(),
  );

  useEffect(() => {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(prefs));
    } catch {
      // ignore
    }
  }, [prefs]);

  const setVerbosity = useCallback((verbosity: ChatVerbosity) => {
    setPrefs({ verbosity, ...PRESETS[verbosity] });
  }, []);

  const setToggle = useCallback(
    (key: keyof Omit<ChatDisplayPrefs, "verbosity">, value: boolean) => {
      setPrefs((prev) => ({ ...prev, [key]: value }));
    },
    [],
  );

  return { prefs, setVerbosity, setToggle, setPrefs };
}
