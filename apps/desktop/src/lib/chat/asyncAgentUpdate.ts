import type { ConversationEntry } from "../../types";

/** Upsert a durable asynchronous agent update before the active response entry. */
export function upsertAsyncAgentUpdate(
  messages: ConversationEntry[],
  activeAssistantId: string,
  id: string,
  content: string,
  createdAt = Date.now(),
): ConversationEntry[] {
  const existing = messages.findIndex((message) => message.id === id);
  if (existing >= 0) {
    return messages.map((message, index) =>
      index === existing ? { ...message, content, delivery: "async" } : message,
    );
  }

  const message: ConversationEntry = {
    id,
    role: "assistant",
    content,
    delivery: "async",
    createdAt,
  };
  const active = messages.findIndex(
    (candidate) => candidate.id === activeAssistantId,
  );
  if (active < 0) return [...messages, message];
  return [...messages.slice(0, active), message, ...messages.slice(active)];
}
