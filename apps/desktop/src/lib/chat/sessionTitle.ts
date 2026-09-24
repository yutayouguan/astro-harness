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

/** 会话标题的展示形态：定时任务会话单独标记，由调用方改用时钟图标表达。 */
export type SessionTitleDisplay = {
  /** 标题来自定时任务会话 */
  isCron: boolean;
  /** 去掉「定时任务 · 」前缀的展示文本 */
  title: string;
};

/**
 * 定时任务会话的标题不再重复写「定时任务」，展示层换成时钟图标；
 * 存储值保持原样，只有展示值变化。
 */
export function sessionTitleDisplay(
  summary: string,
  sessionId: string,
): SessionTitleDisplay {
  const title = visibleSessionTitle(summary, sessionId);
  if (title.startsWith(CRON_TITLE_PREFIX)) {
    return {
      isCron: true,
      title: title.slice(CRON_TITLE_PREFIX.length).trim(),
    };
  }
  // 任务名为空时存储值只剩前缀，且尾部空格已被 trim。
  if (title === CRON_TITLE_PREFIX.trimEnd()) return { isCron: true, title: "" };
  return { isCron: false, title };
}

/**
 * 由用户输入派生一个「立即展示」的兜底标题：折叠空白成单行并截断。
 * 后端 AI 标题尚未生成前，侧栏与顶部先显示它。
 */
export function pendingSessionTitle(text: string, maxChars = 32): string {
  const collapsed = text.trim().split(/\s+/).join(" ");
  if (!collapsed) return "";
  return [...collapsed].slice(0, maxChars).join("").trim();
}
