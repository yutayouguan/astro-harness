export type TaskTerminalOutcome = string | null | undefined;

export type TaskCompletionResolution = {
  failed: boolean;
  error: string | null;
  celebrate: boolean;
};

type ResolveTaskCompletionOptions = {
  outcome: TaskTerminalOutcome;
  terminalError?: string | null;
  hasRenderableOutput: boolean;
  emptyResponseError: string;
};

/**
 * Convert transport terminal state into the UI's completion semantics.
 * A provider-level success with no visible result is still an application error.
 */
export function resolveTaskCompletion({
  outcome,
  terminalError,
  hasRenderableOutput,
  emptyResponseError,
}: ResolveTaskCompletionOptions): TaskCompletionResolution {
  const requiresRenderableOutput = outcome == null || outcome === "success";
  const error =
    terminalError ??
    (outcome === "error"
      ? emptyResponseError
      : requiresRenderableOutput && !hasRenderableOutput
        ? emptyResponseError
        : null);
  const failed = error != null;
  return {
    failed,
    error,
    celebrate: outcome === "success" && !failed,
  };
}

export type ParallelCompletionResolution = TaskCompletionResolution & {
  status: "done" | "error" | "cancelled" | null;
};

/** `null` means the task remains active, currently only for HITL waiting. */
export function resolveParallelTaskCompletion(
  options: ResolveTaskCompletionOptions,
): ParallelCompletionResolution {
  if (options.outcome === "hitl_waiting") {
    return { status: null, failed: false, error: null, celebrate: false };
  }
  if (options.outcome === "interrupt") {
    return { status: "cancelled", failed: false, error: null, celebrate: false };
  }

  const completion = resolveTaskCompletion(options);
  return {
    ...completion,
    status: completion.failed ? "error" : "done",
  };
}

/** A mounted overlay treats its initial trigger as history, not a new event. */
export function shouldStartCompletionCelebration(
  previousTrigger: number,
  nextTrigger: number,
): boolean {
  return nextTrigger > 0 && nextTrigger > previousTrigger;
}
