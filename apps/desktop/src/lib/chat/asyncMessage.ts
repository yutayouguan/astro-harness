import type { ChatMessage } from "../../types";

/** Upsert a durable async assistant message immediately before the active response bubble. */
export function upsertAsyncAssistantMessage(
  messages: ChatMessage[],
  activeAssistantId: string,
  id: string,
  content: string,
  createdAt = Date.now(),
): ChatMessage[] {
  const existing = messages.findIndex((message) => message.id === id);
  if (existing >= 0) {
    return messages.map((message, index) =>
      index === existing ? { ...message, content, delivery: "async" } : message,
    );
  }

  const message: ChatMessage = {
    id,
    role: "assistant",
    content,
    delivery: "async",
    createdAt,
  };
  const active = messages.findIndex((candidate) => candidate.id === activeAssistantId);
  if (active < 0) return [...messages, message];
  return [...messages.slice(0, active), message, ...messages.slice(active)];
}
