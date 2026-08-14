/**
 * 单线程模式（Agent / Plan / Ask）的 follow-up 队列项。
 * MultiTask 不使用此队列（见 chat-mode-scheduling 设计）。
 */

import type { ChatAttachment } from "../../types";

export type QueuedFollowUp = {
  id: string;
  text: string;
  attachments: ChatAttachment[];
  createdAt: number;
};

export const MAX_QUEUED_FOLLOWUPS = 20;

export function newQueuedFollowUpId(): string {
  return `q-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}
