import type { ConversationEntry } from "../../types";

export function findLastUserEntryIndex(entries: ConversationEntry[]): number {
  for (let index = entries.length - 1; index >= 0; index -= 1) {
    const entry = entries[index];
    if (entry?.role === "user" && entry.id !== "welcome") return index;
  }
  return -1;
}

export function findLastUserEntryId(
  entries: ConversationEntry[],
): string | null {
  const index = findLastUserEntryIndex(entries);
  return index >= 0 ? entries[index]!.id : null;
}
