import type { ChatMessage } from "../../types";

export type GroupedAssistantAnswer = {
  reasoning: string;
  reasoningDurationSec?: number;
  text: string;
};

/**
 * Build the optional categorized projection without mutating the canonical
 * event timeline. Canonical aggregate fields win; segments are a recovery
 * fallback for older or partially streamed messages.
 */
export function groupAssistantAnswer(
  message: Pick<
    ChatMessage,
    "content" | "reasoning" | "reasoningDurationSec" | "segments"
  >,
): GroupedAssistantAnswer {
  const reasoningSegments = (message.segments ?? []).filter(
    (segment) => segment.type === "reasoning",
  );
  const textSegments = (message.segments ?? []).filter(
    (segment) => segment.type === "text",
  );
  const durationFromSegments = reasoningSegments.reduce(
    (total, segment) => total + Math.max(0, segment.durationSec ?? 0),
    0,
  );

  return {
    reasoning:
      message.reasoning ?? reasoningSegments.map((segment) => segment.text).join(""),
    reasoningDurationSec:
      message.reasoningDurationSec ??
      (durationFromSegments > 0 ? durationFromSegments : undefined),
    text: message.content || textSegments.map((segment) => segment.text).join(""),
  };
}
