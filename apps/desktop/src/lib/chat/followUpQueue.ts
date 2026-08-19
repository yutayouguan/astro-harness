/**
 * 当前任务繁忙时使用的 follow-up 队列项。
 */

import type { ChatAttachment } from "../../types";

export type QueuedFollowUp = {
  id: string;
  text: string;
  attachments: ChatAttachment[];
  createdAt: number;
  delivery?: "queued" | "steering";
};

export const MAX_QUEUED_FOLLOWUPS = 20;

export function newQueuedFollowUpId(): string {
  return `q-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}
