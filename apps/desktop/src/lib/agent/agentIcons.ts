/** Agent 图标 URL / 首字回退。 */
import { convertFileSrc } from "@tauri-apps/api/core";

export type AgentIconInfo = {
  name: string;
  /** 用于识别默认 workspace Agent 的固定 fallback 图标 */
  id?: string | null;
  is_default?: boolean;
  emoji?: string | null;
  avatar?: string | null;
};

function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** 仅把可显示的图片引用当作图标（本地路径 / http / data URL），纯 emoji 字符不算。 */
export function isAgentIconSrc(value: string | null | undefined): boolean {
  if (!value) return false;
  const v = value.trim();
  if (!v) return false;
  return (
    /^https?:\/\//i.test(v) ||
    v.startsWith("data:image/") ||
    v.startsWith("asset:") ||
    v.startsWith("file:") ||
    v.startsWith("/") ||
    /^[a-zA-Z]:[\\/]/.test(v) ||
    /\.(png|jpe?g|gif|webp|svg|ico)(\?.*)?$/i.test(v)
  );
}

export function resolveAgentIconSrc(agent: AgentIconInfo): string | null {
  const raw = [agent.avatar, agent.emoji].find((v) => isAgentIconSrc(v));
  if (!raw) return null;
  const src = raw!.trim();
  if (
    /^https?:\/\//i.test(src) ||
    src.startsWith("data:image/") ||
    src.startsWith("asset:")
  ) {
    return src;
  }
  if (isTauri()) {
    try {
      return convertFileSrc(src);
    } catch {
      return src;
    }
  }
  return src;
}

export function agentNameInitial(name: string): string {
  const trimmed = name.trim();
  if (!trimmed) return "A";
  return Array.from(trimmed)[0]?.toUpperCase() || "A";
}
