/**
 * MultiTask 并行任务：每条用户消息对应独立 session_id 流式执行。
 * 同 session 禁止并发（后端 pause 重入会 cancel）。
 */

export type ParallelTaskStatus = "running" | "done" | "error" | "cancelled";

export type ParallelChatTask = {
  id: string;
  sessionId: string;
  prompt: string;
  userMessageId: string;
  assistantMessageId: string;
  status: ParallelTaskStatus;
  error?: string;
  createdAt: number;
  finishedAt?: number;
};

/** 同时 running 的上限（不含已结束） */
export const MAX_PARALLEL_RUNNING = 5;

export function newParallelTaskId(): string {
  return `pt-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

export function countRunningParallel(tasks: ParallelChatTask[]): number {
  return tasks.filter((t) => t.status === "running").length;
}
