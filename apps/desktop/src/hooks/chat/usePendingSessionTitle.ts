import { useSyncExternalStore } from "react";
import {
  getPendingSessionTitle,
  subscribeSessionsChanged,
} from "../../lib/chat/sessionManagement";

/**
 * 新建会话在前端侧的兜底标题。后端 `summary`（AI 标题或首条消息 preview）可用前，
 * 顶部标题与侧栏用它先展示用户输入。
 */
export function usePendingSessionTitle(sessionId: string | null): string {
  return useSyncExternalStore(
    subscribeSessionsChanged,
    () => (sessionId ? (getPendingSessionTitle(sessionId) ?? "") : ""),
  );
}
