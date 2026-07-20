/**
 * MultiTask 并行任务：每条用户消息对应独立 session_id 流式执行。
 * 同 session 禁止并发（后端 pause 重入会 cancel）。
 */

import type { PendingInterrupt } from "../../types";

export type ParallelTaskStatus =
  | "running"
  | "waiting"
  | "done"
  | "error"
  | "cancelled";

export type ParallelWorktreeInfo = {
  path: string;
  repoRoot: string;
  branch: string;
};

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
  /** MultiTask git worktree；无仓时为空 */
  worktree?: ParallelWorktreeInfo;
  /** 该 task 会话内未决 HITL（不写入主会话 sessionPendingInterrupts） */
  pendingInterrupts?: PendingInterrupt[];
};

/** 同时占用槽位的上限（running + waiting） */
export const MAX_PARALLEL_RUNNING = 5;

export function newParallelTaskId(): string {
  return `pt-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

/** 仍占用并发槽：执行中或等待用户审批/澄清 */
export function countRunningParallel(tasks: ParallelChatTask[]): number {
  return tasks.filter((t) => t.status === "running" || t.status === "waiting").length;
}

export function isParallelTaskActive(status: ParallelTaskStatus): boolean {
  return status === "running" || status === "waiting";
}

export function countSettledByStatus(tasks: ParallelChatTask[]): {
  done: number;
  error: number;
  cancelled: number;
} {
  let done = 0;
  let error = 0;
  let cancelled = 0;
  for (const t of tasks) {
    if (t.status === "done") done += 1;
    else if (t.status === "error") error += 1;
    else if (t.status === "cancelled") cancelled += 1;
  }
  return { done, error, cancelled };
}

/** 本地汇总 Markdown（不调 LLM） */
export function buildParallelTasksSummaryMarkdown(
  tasks: ParallelChatTask[],
  replyByAssistantId: Map<string, string>,
  maxReplyChars = 400,
): string {
  const lines: string[] = ["## MultiTask 汇总", ""];
  tasks.forEach((task, i) => {
    const n = i + 1;
    lines.push(`### ${n}. [${task.status}] ${task.prompt.trim() || "(empty)"}`);
    if (task.worktree?.path) {
      lines.push(`- worktree: \`${task.worktree.path}\``);
    }
    if (task.error) {
      lines.push(`- error: ${task.error}`);
    }
    const raw = (replyByAssistantId.get(task.assistantMessageId) ?? "").trim();
    if (raw) {
      const clipped =
        raw.length > maxReplyChars ? `${raw.slice(0, maxReplyChars)}…` : raw;
      lines.push("", clipped, "");
    } else {
      lines.push("");
    }
  });
  return lines.join("\n").trimEnd();
}
