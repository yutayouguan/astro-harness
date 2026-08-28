import type { ChatMessage } from "../../types";

export function findLastUserMessageIndex(messages: ChatMessage[]): number {
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const message = messages[index];
    if (message?.role === "user" && message.id !== "welcome") return index;
  }
  return -1;
}

export function findLastUserMessageId(messages: ChatMessage[]): string | null {
  const index = findLastUserMessageIndex(messages);
  return index >= 0 ? messages[index]!.id : null;
}
