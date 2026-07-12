/**
 * 聊天会话本地持久化：消息列表写入 localStorage，附件重字段剥离以免撑爆配额。
 */

import type { ChatMessage } from "../types";

const STORAGE_KEY = "astro.chat.session";
const CLEARED_KEY = "astro.chat.cleared";

/** 持久化的会话快照 */
export type StoredChatSession = {
  sessionId: string | null;
  messages: ChatMessage[];
  updatedAt: number;
};

/** 去掉 previewUrl / dataBase64 等大字段后再存储 */
function stripHeavyFields(messages: ChatMessage[]): ChatMessage[] {
  return messages.map((m) => ({
    ...m,
    attachments: m.attachments?.map((a) => ({
      id: a.id,
      name: a.name,
      mime: a.mime,
      kind: a.kind,
      size: a.size,
      // 不持久化 previewUrl / dataBase64，避免撑爆 localStorage
    })),
  }));
}

/** 是否仅为欢迎占位（不应落盘） */
export function isWelcomeOnly(messages: ChatMessage[]): boolean {
  return messages.length === 0 || (messages.length === 1 && messages[0].id === "welcome");
}

/** 用户是否主动清空过聊天（避免欢迎页写回） */
export function isChatCleared(): boolean {
  try {
    return localStorage.getItem(CLEARED_KEY) === "1";
  } catch {
    return false;
  }
}

/** 标记已清空并删除会话存储 */
export function markChatCleared(): void {
  try {
    localStorage.removeItem(STORAGE_KEY);
    localStorage.setItem(CLEARED_KEY, "1");
  } catch {
    // ignore
  }
}

function clearClearedFlag(): void {
  try {
    localStorage.removeItem(CLEARED_KEY);
  } catch {
    // ignore
  }
}

/** 读取本地会话；已清空或无效则返回 null */
export function loadChatSession(): StoredChatSession | null {
  try {
    if (isChatCleared()) return null;
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as StoredChatSession;
    if (!parsed || !Array.isArray(parsed.messages)) return null;
    if (isWelcomeOnly(parsed.messages)) return null;
    return parsed;
  } catch {
    return null;
  }
}

/** 保存会话；欢迎页不覆盖「已清除」标记 */
export function saveChatSession(
  sessionId: string | null,
  messages: ChatMessage[],
): void {
  try {
    if (isWelcomeOnly(messages)) {
      // 欢迎页不覆盖「已清除」标记，也不写入空会话
      if (!isChatCleared()) {
        localStorage.removeItem(STORAGE_KEY);
      }
      return;
    }
    clearClearedFlag();
    const payload: StoredChatSession = {
      sessionId,
      messages: stripHeavyFields(messages),
      updatedAt: Date.now(),
    };
    localStorage.setItem(STORAGE_KEY, JSON.stringify(payload));
  } catch {
    // quota / private mode
  }
}

/** 清空会话（同 {@link markChatCleared}） */
export function clearChatSession(): void {
  markChatCleared();
}
