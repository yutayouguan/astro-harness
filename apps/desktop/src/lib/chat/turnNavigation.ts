import type { ConversationEntry } from "../../types";

export type TurnPreview = {
  id: string;
  targetEntryId: string;
  entryIds: string[];
  question: string;
  answer: string;
};

export function compactEntryPreview(content: string, limit = 180): string {
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
export function buildTurnPreviews(
  entries: ConversationEntry[],
  fallbackQuestion: string,
  fallbackAnswer: string,
): TurnPreview[] {
  const turns: TurnPreview[] = [];

  for (const entry of entries) {
    if (entry.role === "user") {
      turns.push({
        id: entry.id,
        targetEntryId: entry.id,
        entryIds: [entry.id],
        question: compactEntryPreview(entry.content, 72) || fallbackQuestion,
        answer: "",
      });
      continue;
    }

    const turn = turns[turns.length - 1];
    if (!turn) continue;
    turn.entryIds.push(entry.id);
    const answer = compactEntryPreview(entry.content || entry.reasoning || "");
    if (answer) turn.answer = answer;
  }

  return turns.map((turn) => ({
    ...turn,
    answer: turn.answer || fallbackAnswer,
  }));
}
