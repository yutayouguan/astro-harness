/** Project sidebar sizing rules shared by the shell hook and focused tests. */
export const SIDEBAR_WIDTH_KEY = "astro.sidebarWidth";
export const SIDEBAR_DEFAULT_WIDTH = 280;
export const SIDEBAR_MIN_WIDTH = 200;
export const SIDEBAR_MAX_WIDTH = 420;
export const SIDEBAR_MIN_CONTENT_WIDTH = 440;
// Keep roughly 680px available for the primary pane beside the default sidebar.
export const SIDEBAR_COMPACT_MAX_WIDTH = 960;

export function shouldUseCompactSidebar(containerWidth: number): boolean {
  return (
    Number.isFinite(containerWidth) &&
    containerWidth > 0 &&
    containerWidth <= SIDEBAR_COMPACT_MAX_WIDTH
  );
}

export function maxSidebarWidth(containerWidth: number): number {
  if (!Number.isFinite(containerWidth)) return SIDEBAR_MAX_WIDTH;
  return Math.max(
    0,
    Math.min(
      SIDEBAR_MAX_WIDTH,
      Math.floor(containerWidth) - SIDEBAR_MIN_CONTENT_WIDTH,
    ),
  );
}

export function clampSidebarWidth(
  width: number,
  containerWidth = Number.POSITIVE_INFINITY,
): number {
  const maxWidth = maxSidebarWidth(containerWidth);
  const minWidth = Math.min(SIDEBAR_MIN_WIDTH, maxWidth);
  const fallback = Math.min(SIDEBAR_DEFAULT_WIDTH, maxWidth);
  const candidate = Number.isFinite(width) ? width : fallback;
  return Math.round(Math.min(maxWidth, Math.max(minWidth, candidate)));
}

export function parseStoredSidebarWidth(raw: string | null): number {
  if (raw == null || raw.trim() === "") return SIDEBAR_DEFAULT_WIDTH;
  return clampSidebarWidth(Number(raw));
}
