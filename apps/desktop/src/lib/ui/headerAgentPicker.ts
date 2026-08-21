/** 标题栏 AgentPicker 显隐矩阵（与 nav 解耦，便于单测）。 */

export const HEADER_AGENT_PICKER_NAV_IDS = [
  "chat",
  "files",
  "skills",
  "loop",
] as const;

export type HeaderAgentPickerNavId =
  (typeof HEADER_AGENT_PICKER_NAV_IDS)[number];

const HEADER_AGENT_PICKER_NAV_SET = new Set<string>(HEADER_AGENT_PICKER_NAV_IDS);

export function showsHeaderAgentPicker(nav: string): boolean {
  return HEADER_AGENT_PICKER_NAV_SET.has(nav);
}
