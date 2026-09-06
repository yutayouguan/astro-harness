import type { AsyncUserInputQuestion, ConversationEntry } from "../../types";

/** Upsert a durable asynchronous agent update before the active response entry. */
export function upsertAsyncAgentUpdate(
  messages: ConversationEntry[],
  activeAssistantId: string,
  id: string,
  content: string,
  questions?: AsyncUserInputQuestion[],
  createdAt = Date.now(),
): ConversationEntry[] {
  const existing = messages.findIndex((message) => message.id === id);
  if (existing >= 0) {
    return messages.map((message, index) =>
      index === existing
        ? {
            ...message,
            content,
            delivery: "async",
            ...(questions ? { asyncQuestions: questions } : {}),
          }
        : message,
    );
  }

  const message: ConversationEntry = {
    id,
    role: "assistant",
    content,
    delivery: "async",
    ...(questions ? { asyncQuestions: questions } : {}),
    createdAt,
  };
  const active = messages.findIndex(
    (candidate) => candidate.id === activeAssistantId,
  );
  if (active < 0) return [...messages, message];
  return [...messages.slice(0, active), message, ...messages.slice(active)];
}

/** Only the newest unanswered asynchronous question group remains interactive. */
export function pendingAsyncQuestionsAt(
  messages: ConversationEntry[],
  index: number,
): AsyncUserInputQuestion[] | undefined {
  const questions = messages[index]?.asyncQuestions;
  if (!questions?.length) return undefined;
  const supersededOrAnswered = messages
    .slice(index + 1)
    .some(
      (message) =>
        message.role === "user" || Boolean(message.asyncQuestions?.length),
    );
  return supersededOrAnswered ? undefined : questions;
}
