/**
 * 聊天会话本地持久化：消息列表写入 localStorage，附件重字段剥离以免撑爆配额。
 */

import type { ConversationEntry, PendingInterrupt } from "../../types";
import type { ContextUsageSnapshot } from "./contextUsage";

const STORAGE_KEY = "astro.chat.session";
const CLEARED_KEY = "astro.chat.cleared";
const USAGE_BY_SESSION_KEY = "astro.chat.contextUsageBySession";

/** 按会话缓存最近一次 context_usage（切换会话时可即时恢复） */
export function loadContextUsageForSession(
  sessionId: string | null | undefined,
): ContextUsageSnapshot | null {
  if (!sessionId) return null;
  try {
    const raw = localStorage.getItem(USAGE_BY_SESSION_KEY);
    if (!raw) return null;
    const map = JSON.parse(raw) as Record<string, ContextUsageSnapshot>;
    const snap = map?.[sessionId];
    if (!snap || typeof snap.totalTokens !== "number") return null;
    return {
      ...snap,
      estimatedTotalTokens: snap.estimatedTotalTokens ?? snap.totalTokens,
      source:
        snap.source === "provider_reported" ||
        snap.source === "provider_recomputed"
          ? snap.source
          : "local_estimate",
    };
  } catch {
    return null;
  }
}

export function saveContextUsageForSession(
  sessionId: string | null | undefined,
  usage: ContextUsageSnapshot | null,
): void {
  if (!sessionId) return;
  try {
    const raw = localStorage.getItem(USAGE_BY_SESSION_KEY);
    const map = (raw ? JSON.parse(raw) : {}) as Record<
      string,
      ContextUsageSnapshot
    >;
    if (!usage) {
      delete map[sessionId];
    } else {
      map[sessionId] = usage;
    }
    // 简单上限，避免无限增长
    const keys = Object.keys(map);
    if (keys.length > 80) {
      for (const k of keys.slice(0, keys.length - 80)) {
        delete map[k];
      }
    }
    localStorage.setItem(USAGE_BY_SESSION_KEY, JSON.stringify(map));
  } catch {
    // ignore
  }
}

/** 持久化的会话快照 */
export type StoredChatSession = {
  sessionId: string | null;
  messages: ConversationEntry[];
  /** 未决 HITL interrupt（重载后仍禁用普通发送） */
  pendingInterrupts?: PendingInterrupt[];
  /** 最近一次后端 context_usage 快照（真实窗口与分层占用） */
  contextUsage?: ContextUsageSnapshot;
  ephemeral?: boolean;
  parentSessionId?: string;
  excludedTurnCount?: number;
  updatedAt: number;
};

/** 去掉 previewUrl / dataBase64 等大字段后再存储 */
function stripHeavyFields(messages: ConversationEntry[]): ConversationEntry[] {
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
export function isWelcomeOnly(messages: ConversationEntry[]): boolean {
  return (
    messages.length === 0 ||
    (messages.length === 1 && messages[0].id === "welcome")
  );
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

function peekStoredSession(): StoredChatSession | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as StoredChatSession;
    if (!parsed || !Array.isArray(parsed.messages)) return null;
    return parsed;
  } catch {
    return null;
  }
}

/**
 * 保存会话；欢迎页不覆盖「已清除」标记。
 * `contextUsage`：传入对象则写入；传 `null` 清除；省略则保留上次记录。
 */
export function saveChatSession(
  sessionId: string | null,
  messages: ConversationEntry[],
  pendingInterrupts: PendingInterrupt[] = [],
  contextUsage?: ContextUsageSnapshot | null,
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
    const prev = peekStoredSession();
    const usage =
      contextUsage === undefined
        ? prev?.contextUsage
        : (contextUsage ?? undefined);
    const payload: StoredChatSession = {
      sessionId,
      messages: stripHeavyFields(messages),
      pendingInterrupts:
        pendingInterrupts.length > 0 ? pendingInterrupts : undefined,
      contextUsage: usage,
      ephemeral: prev?.sessionId === sessionId ? prev.ephemeral : undefined,
      parentSessionId:
        prev?.sessionId === sessionId ? prev.parentSessionId : undefined,
      excludedTurnCount:
        prev?.sessionId === sessionId ? prev.excludedTurnCount : undefined,
      updatedAt: Date.now(),
    };
    localStorage.setItem(STORAGE_KEY, JSON.stringify(payload));
    if (sessionId && contextUsage !== undefined) {
      saveContextUsageForSession(sessionId, contextUsage);
    }
  } catch {
    // quota / private mode
  }
}

/** 记录当前会话的临时 Side 属性，供刷新/异常退出恢复时执行清理。 */
export function saveEphemeralSessionMeta(
  sessionId: string,
  parentSessionId: string | null | undefined,
  excludedTurnCount: number,
): void {
  try {
    const current = peekStoredSession();
    if (!current || current.sessionId !== sessionId) return;
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({
        ...current,
        ephemeral: true,
        parentSessionId: parentSessionId ?? undefined,
        excludedTurnCount,
        updatedAt: Date.now(),
      } satisfies StoredChatSession),
    );
  } catch {
    // private mode / quota
  }
}

/** 清空会话（同 {@link markChatCleared}） */
export function clearChatSession(): void {
  markChatCleared();
}
