/** Skills 面板分页与缓存辅助。 */

/** 分页追加后若没有新增唯一项，应视为没有更多 */
export function pageHasMore(
  fetchedCount: number,
  pageSize: number,
  newlyAddedCount: number,
): boolean {
  if (newlyAddedCount <= 0) return false;
  return fetchedCount >= pageSize;
}

/** 滚动容器是否已接近底部，用于触发下一页。 */
export function isNearScrollEnd(
  metrics: {
    scrollTop: number;
    scrollHeight: number;
    clientHeight: number;
  },
  threshold = 160,
): boolean {
  if (metrics.clientHeight <= 0 || metrics.scrollHeight <= 0) return false;
  const remaining =
    metrics.scrollHeight - metrics.scrollTop - metrics.clientHeight;
  return remaining <= Math.max(0, threshold);
}

/** 商店列表会话缓存 TTL（过期后切回 Tab 会静默刷新） */
export const STORE_CACHE_TTL_MS = 5 * 60 * 1000;

/** 已安装 / 本机列表 TTL（同 Agent 切 Tab 时在此窗口内可跳过请求） */
export const LOCAL_SKILLS_TTL_MS = 60 * 1000;

export function storeCacheKey(
  query: string,
  sort = "all",
  category = "all",
  apiKey = "all",
): string {
  return [query.trim().toLowerCase(), sort, category, apiKey].join("\u0000");
}

export function isStoreCacheFresh(
  fetchedAt: number,
  now = Date.now(),
  ttlMs = STORE_CACHE_TTL_MS,
): boolean {
  return now - fetchedAt < ttlMs;
}
