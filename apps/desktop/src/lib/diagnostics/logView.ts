import type { AgentLogLine } from "../settings/diagnosticsModel.ts";

/** Match a selected snapshot after live refresh replaces the row objects. */
export function sameDiagnosticLog(
  a: AgentLogLine | null,
  b: AgentLogLine,
): boolean {
  return (
    a != null &&
    a.source === b.source &&
    a.timestamp === b.timestamp &&
    a.raw === b.raw
  );
}

export type DiagnosticLogTimeRange =
  | "15m"
  | "1h"
  | "24h"
  | "7d"
  | "all"
  | "custom";

const RANGE_MS: Partial<Record<DiagnosticLogTimeRange, number>> = {
  "15m": 15 * 60 * 1_000,
  "1h": 60 * 60 * 1_000,
  "24h": 24 * 60 * 60 * 1_000,
  "7d": 7 * 24 * 60 * 60 * 1_000,
};

function parseLocalDateTime(value: string): number | null {
  if (!value.trim()) return null;
  const parsed = new Date(value).getTime();
  return Number.isFinite(parsed) ? parsed : null;
}

export function diagnosticLogTimeBounds(
  range: DiagnosticLogTimeRange,
  customStart: string,
  customEnd: string,
  nowMs = Date.now(),
): { sinceMs: number | null; untilMs: number | null } {
  if (range === "all") return { sinceMs: null, untilMs: null };
  if (range === "custom") {
    return {
      sinceMs: parseLocalDateTime(customStart),
      untilMs: parseLocalDateTime(customEnd),
    };
  }
  return { sinceMs: nowMs - (RANGE_MS[range] ?? 0), untilMs: null };
}

export function toLocalDateTimeInput(timestampMs: number): string {
  const date = new Date(timestampMs);
  const local = new Date(timestampMs - date.getTimezoneOffset() * 60_000);
  return local.toISOString().slice(0, 16);
}

export function formatDiagnosticTimestamp(
  timestamp: string,
  locale: "zh" | "en",
): string {
  const date = new Date(timestamp);
  if (Number.isNaN(date.getTime())) return timestamp;
  return new Intl.DateTimeFormat(locale === "zh" ? "zh-CN" : "en-US", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  }).format(date);
}

export function presentDiagnosticMessage(message: string): string {
  return message
    .split("\\r\\n")
    .join("\n")
    .split("\\n")
    .join("\n")
    .split("\\t")
    .join("  ");
}
