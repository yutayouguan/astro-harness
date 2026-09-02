/** Browser dock sizing rules shared by the component and focused tests. */
export const BROWSER_DOCK_WIDTH_KEY = "astro.browserDockWidth";
export const BROWSER_DOCK_DEFAULT_WIDTH = 680;
export const BROWSER_DOCK_MIN_WIDTH = 420;
export const BROWSER_DOCK_MAX_WIDTH = 1_200;
export const BROWSER_DOCK_MAIN_MIN_WIDTH = 320;
export const BROWSER_DOCK_OVERLAY_BREAKPOINT = 840;

export function maxBrowserDockWidth(
  containerWidth: number,
  overlayLayout = false,
): number {
  if (!Number.isFinite(containerWidth)) return BROWSER_DOCK_MAX_WIDTH;
  const width = Math.max(0, Math.floor(containerWidth));
  if (overlayLayout) return width;
  return Math.max(
    0,
    Math.min(BROWSER_DOCK_MAX_WIDTH, width - BROWSER_DOCK_MAIN_MIN_WIDTH),
  );
}

export function clampBrowserDockWidth(
  width: number,
  containerWidth = Number.POSITIVE_INFINITY,
  overlayLayout = false,
): number {
  const maxWidth = maxBrowserDockWidth(containerWidth, overlayLayout);
  const minWidth = Math.min(BROWSER_DOCK_MIN_WIDTH, maxWidth);
  const fallback = Math.min(BROWSER_DOCK_DEFAULT_WIDTH, maxWidth);
  const candidate = Number.isFinite(width) ? width : fallback;
  return Math.round(Math.min(maxWidth, Math.max(minWidth, candidate)));
}

export function parseStoredBrowserDockWidth(raw: string | null): number {
  if (raw == null || raw.trim() === "") return BROWSER_DOCK_DEFAULT_WIDTH;
  return clampBrowserDockWidth(Number(raw));
}
