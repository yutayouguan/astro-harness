import type { PendingInterrupt, UiSurface } from "../../types";
import { parseActivityOperations } from "./chatTimeline.ts";

export type ParsedHitlRunFinished = {
  interrupts: PendingInterrupt[];
  surface?: UiSurface;
};

function parseObject(raw: unknown): Record<string, unknown> | null {
  if (typeof raw !== "string" || !raw.trim()) return null;
  try {
    const value = JSON.parse(raw) as unknown;
    return value != null && typeof value === "object" && !Array.isArray(value)
      ? (value as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
}

export function parseHitlRunFinished(
  interruptsJson: string | undefined,
  assistantMessageId: string,
): ParsedHitlRunFinished {
  let records: Record<string, unknown>[] = [];
  try {
    const value = JSON.parse(interruptsJson || "[]") as unknown;
    if (Array.isArray(value)) {
      records = value.filter(
        (item): item is Record<string, unknown> =>
          item != null && typeof item === "object" && !Array.isArray(item),
      );
    }
  } catch {
    return { interrupts: [] };
  }

  const interrupts: PendingInterrupt[] = records.flatMap((record) => {
      const id = String(record.id ?? "");
      if (!id) return [];
      const schema = parseObject(record.response_schema_json);
      const interrupt: PendingInterrupt = {
        id,
        reason: String(record.reason ?? ""),
        assistantMessageId,
      };
      if (typeof record.message === "string") interrupt.message = record.message;
      if (schema) interrupt.responseSchema = schema;
      return [interrupt];
    });

  const surfaceRecord = records.find((record) => {
    const metadata = parseObject(record.metadata_json);
    return Array.isArray(metadata?.operations);
  });
  if (!surfaceRecord) return { interrupts };
  const operations = parseActivityOperations(
    typeof surfaceRecord.metadata_json === "string" ? surfaceRecord.metadata_json : undefined,
  );
  const stableId = String(surfaceRecord.tool_call_id ?? surfaceRecord.id ?? "hitl");
  return {
    interrupts,
    surface: {
      messageId: `a2ui-surface-${stableId}`,
      activityType: "a2ui-surface",
      operations,
      status: "active",
      interrupts: interrupts.map(({ id, reason, message, responseSchema }) => ({
        id,
        reason,
        message,
        responseSchema,
      })),
    },
  };
}
