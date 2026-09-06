const CRON_TITLE_PREFIX = "定时任务 · ";

/**
 * 会话标题在 SQLite 中需要唯一，同名标题可能被补上短 session id。
 * 定时任务在侧栏只展示稳定的任务名，存储值保持不变。
 */
export function visibleSessionTitle(
  summary: string,
  sessionId: string,
): string {
  const title = summary.trim();
  const shortId = sessionId.trim().slice(0, 8);
  if (!title.startsWith(CRON_TITLE_PREFIX) || !shortId) return title;

  const suffix = ` · ${shortId}`;
  return title.endsWith(suffix)
    ? title.slice(0, -suffix.length).trimEnd()
    : title;
}
