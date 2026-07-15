/** Skills 面板懒加载辅助。 */
/** 在线技能列表：触底惰性加载的边缘触发状态机 */

export type LazyLoadGate = {
  /** 上一帧哨兵是否已进入视口（用于边沿检测，避免常驻视口时连环请求） */
  wasIntersecting: boolean;
};

/**
 * @param suppressInitial 为 true 时，首帧即使哨兵已在视口内也不自动加载，
 *   需用户先滚离再靠近（或点击「加载更多」）才会请求下一页。
 */
export function createLazyLoadGate(opts?: {
  suppressInitial?: boolean;
}): LazyLoadGate {
  return { wasIntersecting: Boolean(opts?.suppressInitial) };
}

export type LazyLoadDecision = {
  /** 是否应发起一次 loadMore */
  shouldLoad: boolean;
  next: LazyLoadGate;
};

/**
 * 仅在「未相交 → 相交」上升沿触发加载。
 * 哨兵一直可见时不会连环拉取；滚离后再靠近才会加载下一页。
 */
export function decideLazyLoad(
  gate: LazyLoadGate,
  opts: {
    isIntersecting: boolean;
    hasMore: boolean;
    isLoading: boolean;
  },
): LazyLoadDecision {
  const { isIntersecting, hasMore, isLoading } = opts;

  if (!isIntersecting) {
    return {
      shouldLoad: false,
      next: { wasIntersecting: false },
    };
  }

  const risingEdge = !gate.wasIntersecting;
  const shouldLoad = risingEdge && hasMore && !isLoading;

  return {
    shouldLoad,
    next: { wasIntersecting: true },
  };
}

/** 分页追加后若没有新增唯一项，应视为没有更多 */
export function pageHasMore(
  fetchedCount: number,
  pageSize: number,
  newlyAddedCount: number,
): boolean {
  if (newlyAddedCount <= 0) return false;
  return fetchedCount >= pageSize;
}

/** 商店列表会话缓存 TTL（过期后切回 Tab 会静默刷新） */
export const STORE_CACHE_TTL_MS = 5 * 60 * 1000;

/** 已安装 / 本机列表 TTL（同 Agent 切 Tab 时在此窗口内可跳过请求） */
export const LOCAL_SKILLS_TTL_MS = 60 * 1000;

export function storeCacheKey(storeId: string, query: string): string {
  return `${storeId}\0${query.trim().toLowerCase()}`;
}

export function isStoreCacheFresh(
  fetchedAt: number,
  now = Date.now(),
  ttlMs = STORE_CACHE_TTL_MS,
): boolean {
  return now - fetchedAt < ttlMs;
}
