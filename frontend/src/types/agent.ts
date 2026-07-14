/**
 * Agent 信息 DTO 与 id 规范化（与 memory crate 约定对齐）。
 */

/** 侧栏 / 选择器展示的 Agent */
export type AgentInfo = {
  id: string;
  name: string;
  path: string;
  is_default: boolean;
  is_active: boolean;
  emoji?: string | null;
  avatar?: string | null;
  vibe?: string | null;
};

/**
 * 将 `default` / 空 id 规范为 `workspace`（默认 Agent 工作区约定）。
 */
export function normalizeAgentId(id: string | null | undefined): string {
  if (!id || id === "default") return "workspace";
  return id;
}
