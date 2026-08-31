import type { ChatActivityStatus } from "../../types";

const WAITING_PHASES = new Set([
  "waiting",
  "pending",
  "pending_approval",
  "approval_required",
  "input_required",
]);
const RETRYING_PHASES = new Set(["retry", "retrying"]);
const PARTIAL_PHASES = new Set(["partial", "partially_completed"]);
const ERROR_PHASES = new Set(["error", "failed", "timeout", "timed_out"]);
const INTERRUPTED_PHASES = new Set(["cancelled", "canceled", "interrupted"]);
const DONE_PHASES = new Set(["completed", "done", "succeeded", "success"]);

/** Map protocol/provider tool phases onto the stable chat activity state model. */
export function resolveToolActivityStatus(
  phase: string | null | undefined,
  result: string | null | undefined,
): ChatActivityStatus {
  const normalized = phase?.trim().toLowerCase() ?? "";
  if (WAITING_PHASES.has(normalized)) return "waiting";
  if (RETRYING_PHASES.has(normalized)) return "retrying";
  if (PARTIAL_PHASES.has(normalized)) return "partial";
  if (ERROR_PHASES.has(normalized)) return "error";
  if (INTERRUPTED_PHASES.has(normalized)) return "interrupted";
  if (DONE_PHASES.has(normalized) || result?.trim()) return "done";
  return "running";
}

export function isLiveActivityStatus(status: ChatActivityStatus | undefined): boolean {
  return status === "waiting" || status === "running" || status === "retrying";
}

export function isSettledActivityStatus(
  status: ChatActivityStatus | undefined,
): boolean {
  return (
    status === "done" ||
    status === "partial" ||
    status === "error" ||
    status === "interrupted"
  );
}
