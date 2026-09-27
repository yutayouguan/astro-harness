/**
 * 定时任务会话的归属判定。
 *
 * 会话 id 由执行层按 `cron-{job_id}` 固定生成（见 agent-core `cron_session_id`），
 * 所以侧栏可以只凭会话 id 反查任务，用来解释「这条定时任务会话为什么找不到任务」。
 */

/** 定时任务会话 id 前缀；与 agent-core `cron_session_id` 保持一致。 */
export const CRON_SESSION_ID_PREFIX = "cron-";

/** 定时任务会话的归属状态。 */
export type CronOwnerState =
  /** 任务仍在调度列表中（启用或暂停）。 */
  | "active"
  /** 任务已归档：定义保留，可恢复。 */
  | "archived"
  /** 任务已删除：会话与历史仍在，但已无从属任务。 */
  | "missing";

/** 从 `cron-{jobId}` 会话 id 解析任务 id；非定时任务会话返回 null。 */
export function cronJobIdFromSessionId(sessionId: string): string | null {
  const id = sessionId.trim();
  if (!id.startsWith(CRON_SESSION_ID_PREFIX)) return null;
  const jobId = id.slice(CRON_SESSION_ID_PREFIX.length).trim();
  return jobId || null;
}

/**
 * 解析给定定时任务会话的归属状态。
 *
 * 只统计传入的会话 id，非定时任务会话直接忽略，避免调用方再过滤一遍。
 */
export function resolveCronOwnerStates(
  sessionIds: Iterable<string>,
  jobs: ReadonlyArray<{ id: string; archived_at?: string | null }>,
): Map<string, CronOwnerState> {
  const byJobId = new Map<string, CronOwnerState>();
  for (const job of jobs) {
    byJobId.set(job.id, job.archived_at ? "archived" : "active");
  }

  const states = new Map<string, CronOwnerState>();
  for (const sessionId of sessionIds) {
    const jobId = cronJobIdFromSessionId(sessionId);
    if (!jobId) continue;
    states.set(sessionId, byJobId.get(jobId) ?? "missing");
  }
  return states;
}

/** 归属状态是否需要提示用户（正常调度中的任务不额外标注）。 */
export function cronOwnerNeedsAttention(
  state: CronOwnerState | undefined,
): state is "archived" | "missing" {
  return state === "archived" || state === "missing";
}
