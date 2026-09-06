export function nextBrowserPreviewRevision(
  previous: number | null | undefined,
  now = Date.now(),
): number {
  return Math.max(now, (previous ?? 0) + 1);
}
