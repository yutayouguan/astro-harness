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
