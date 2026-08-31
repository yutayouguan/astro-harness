/** 会话未读标记（本地）；点开会话后清除。 */

const STORAGE_KEY = "astro.session.unread.v1";

function readSet(): Set<string> {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return new Set();
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed)) return new Set();
    return new Set(
      parsed.filter(
        (id): id is string => typeof id === "string" && id.length > 0,
      ),
    );
  } catch {
    return new Set();
  }
}

function writeSet(ids: Set<string>) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify([...ids]));
  } catch {
    // ignore quota / private mode
  }
}

export function listUnreadSessionIds(): string[] {
  return [...readSet()];
}

export function isSessionUnread(sessionId: string): boolean {
  return readSet().has(sessionId);
}

export function markSessionUnread(sessionId: string) {
  const id = sessionId.trim();
  if (!id) return;
  const next = readSet();
  if (next.has(id)) return;
  next.add(id);
  writeSet(next);
  window.dispatchEvent(new CustomEvent("astro:session-unread-changed"));
}

export function clearSessionUnread(sessionId: string) {
  const id = sessionId.trim();
  if (!id) return;
  const next = readSet();
  if (!next.delete(id)) return;
  writeSet(next);
  window.dispatchEvent(new CustomEvent("astro:session-unread-changed"));
}

export function subscribeSessionUnread(listener: () => void): () => void {
  const onStorage = (e: StorageEvent) => {
    if (e.key === STORAGE_KEY) listener();
  };
  window.addEventListener("astro:session-unread-changed", listener);
  window.addEventListener("storage", onStorage);
  return () => {
    window.removeEventListener("astro:session-unread-changed", listener);
    window.removeEventListener("storage", onStorage);
  };
}
