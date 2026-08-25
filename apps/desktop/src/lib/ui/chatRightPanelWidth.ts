/** Chat right-panel sizing rules shared by the component and focused tests. */
export const CHAT_RIGHT_PANEL_WIDTH_KEY = "astro.chatRightPanelWidth";
export const CHAT_RIGHT_PANEL_DEFAULT_WIDTH = 360;
export const CHAT_RIGHT_PANEL_MIN_WIDTH = 320;
export const CHAT_RIGHT_PANEL_MAX_WIDTH = 720;
export const CHAT_RIGHT_PANEL_EDGE_GAP = 20;

export function maxChatRightPanelWidth(containerWidth: number): number {
  if (!Number.isFinite(containerWidth)) return CHAT_RIGHT_PANEL_MAX_WIDTH;
  return Math.max(
    0,
    Math.min(CHAT_RIGHT_PANEL_MAX_WIDTH, Math.floor(containerWidth) - CHAT_RIGHT_PANEL_EDGE_GAP),
  );
}

/** Keep the panel usable while preserving the existing 20px gap on narrow layouts. */
export function clampChatRightPanelWidth(width: number, containerWidth = Number.POSITIVE_INFINITY): number {
  const maxWidth = maxChatRightPanelWidth(containerWidth);
  const minWidth = Math.min(CHAT_RIGHT_PANEL_MIN_WIDTH, maxWidth);
  const fallback = Math.min(CHAT_RIGHT_PANEL_DEFAULT_WIDTH, maxWidth);
  const candidate = Number.isFinite(width) ? width : fallback;
  return Math.round(Math.min(maxWidth, Math.max(minWidth, candidate)));
}

export function parseStoredChatRightPanelWidth(raw: string | null): number {
  if (raw == null || raw.trim() === "") return CHAT_RIGHT_PANEL_DEFAULT_WIDTH;
  return clampChatRightPanelWidth(Number(raw));
}
