import type { ChatMessage } from "../../types";

export type ChatTurnPreview = {
  id: string;
  targetMessageId: string;
  messageIds: string[];
  question: string;
  answer: string;
};

export function compactMessagePreview(content: string, limit = 180): string {
  const plain = content
    .replace(
      /<tool(?:_|\s+)call\b[^>]*>[\s\S]*?<\/tool(?:_|\s+)call\s*>/gi,
      " ",
    )
    .replace(/<tool(?:_|\s+)call\b[^>]*>[\s\S]*$/gi, " ")
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/`([^`]*)`/g, "$1")
    .replace(/!\[[^\]]*\]\([^)]*\)/g, " ")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/[#>*_~|\-]+/g, " ")
    .replace(/\s+/g, " ")
    .trim();
  if (plain.length <= limit) return plain;
  return `${plain.slice(0, Math.max(0, limit - 1)).trimEnd()}…`;
}

/** Group the transcript into user-question / assistant-answer turns for navigation. */
export function buildChatTurnPreviews(
  messages: ChatMessage[],
  fallbackQuestion: string,
  fallbackAnswer: string,
): ChatTurnPreview[] {
  const turns: ChatTurnPreview[] = [];

  for (const message of messages) {
    if (message.role === "user") {
      turns.push({
        id: message.id,
        targetMessageId: message.id,
        messageIds: [message.id],
        question:
          compactMessagePreview(message.content, 72) || fallbackQuestion,
        answer: "",
      });
      continue;
    }

    const turn = turns[turns.length - 1];
    if (!turn) continue;
    turn.messageIds.push(message.id);
    const answer = compactMessagePreview(
      message.content || message.reasoning || "",
    );
    if (answer) turn.answer = answer;
  }

  return turns.map((turn) => ({
    ...turn,
    answer: turn.answer || fallbackAnswer,
  }));
}
