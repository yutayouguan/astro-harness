/**
 * 聊天思考强度偏好：读写 localStorage，兼容旧版 `{ enabled, effort }`。
 */

import { useCallback, useEffect, useState } from "react";
import {
  type ChatThinkingPrefs,
  type ThinkingLevel,
  thinkingPrefsEnabled,
} from "../lib/thinkingPrefs";

const STORAGE_KEY = "astro.chat.thinking";

const DEFAULT: ChatThinkingPrefs = { level: "high" };

function readStored(): ChatThinkingPrefs {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return { ...DEFAULT };
    const parsed = JSON.parse(raw) as Partial<ChatThinkingPrefs> & {
      enabled?: boolean;
      effort?: string;
    };
    if (
      parsed.level === "off" ||
      parsed.level === "low" ||
      parsed.level === "high" ||
      parsed.level === "max"
    ) {
      return { level: parsed.level };
    }
    // 兼容旧版 { enabled, effort }
    if (parsed.enabled === false) return { level: "off" };
    if (parsed.effort === "max") return { level: "max" };
    return { level: "high" };
  } catch {
    return { ...DEFAULT };
  }
}

/** @returns 当前偏好、是否启用思考，以及 setters */
export function useChatThinkingPrefs() {
  const [prefs, setPrefs] = useState<ChatThinkingPrefs>(() => readStored());

  useEffect(() => {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(prefs));
    } catch {
      /* ignore */
    }
  }, [prefs]);

  const setLevel = useCallback((level: ThinkingLevel) => {
    setPrefs({ level });
  }, []);

  const toggleEnabled = useCallback(() => {
    setPrefs((p) => ({
      level: p.level === "off" ? "high" : "off",
    }));
  }, []);

  return {
    thinkingPrefs: prefs,
    thinkingEnabled: thinkingPrefsEnabled(prefs),
    setLevel,
    toggleEnabled,
  };
}
