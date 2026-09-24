/** 会话列表变更与安全删除辅助。 */

export type SessionListKind = "active" | "archived";

export const SESSIONS_CHANGED_EVENT = "astro:sessions-changed";

function isBrowser(): boolean {
  return typeof window !== "undefined";
}

export function dispatchSessionsChanged(): void {
  if (!isBrowser()) return;
  window.dispatchEvent(new Event(SESSIONS_CHANGED_EVENT));
}

export function subscribeSessionsChanged(listener: () => void): () => void {
  if (!isBrowser()) return () => {};
  window.addEventListener(SESSIONS_CHANGED_EVENT, listener);
  return () => window.removeEventListener(SESSIONS_CHANGED_EVENT, listener);
}

/** 新建会话在 AI 标题落库前的前端兜底标题（sessionId → title）。 */
const pendingSessionTitles = new Map<string, string>();

/** 记录一个会话的前端兜底标题；只设置，不触发列表刷新（由调用方决定）。 */
export function setPendingSessionTitle(sessionId: string, title: string): void {
  const trimmed = title.trim();
  if (!sessionId || !trimmed) return;
  pendingSessionTitles.set(sessionId, trimmed);
}

/** 读取会话的前端兜底标题；后端已返回 summary 时调用方应优先用 summary。 */
export function getPendingSessionTitle(sessionId: string): string | undefined {
  return pendingSessionTitles.get(sessionId);
}

export async function deleteManagedSession(
  sessionId: string,
  activeSessionId: string | null,
  invokeDelete: () => Promise<void>,
  clearActive: () => Promise<void>,
): Promise<void> {
  await invokeDelete();
  if (sessionId === activeSessionId) {
    await clearActive();
  }
  dispatchSessionsChanged();
}
