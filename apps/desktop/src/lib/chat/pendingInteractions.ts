import { parseClarifySteps } from "../../a2ui/clarifySteps.ts";
export type InteractionAction = {
  id: string;
  label: string;
  payload: Record<string, unknown>;
  persistent: boolean;
};
export type PendingInteraction = {
  key: string;
  sessionId: string;
  turnId: string;
  requestId: string;
  toolCallId: string;
  kind: string;
  message: string;
  operations: unknown;
  responseSchema: unknown;
  actions: InteractionAction[];
  expiresAt: string;
  serverName: string | null;
  generation: number | null;
};
export type InteractionTask = {
  sessionId: string;
  turnId: string;
  title: string;
  project: string;
  parentSessionId: string | null;
  status: string;
};
export type InteractionSnapshot = {
  epoch: string;
  revision: number;
  tasks: InteractionTask[];
  requests: PendingInteraction[];
};
export type InteractionState = {
  connected: boolean;
  snapshot: InteractionSnapshot;
  selected: string | null;
  retiredEpochs?: string[];
};
export const EMPTY_INTERACTIONS: InteractionState = {
  connected: false,
  snapshot: { epoch: "", revision: 0, tasks: [], requests: [] },
  selected: null,
};
export function acceptInteractions(
  current: InteractionState,
  next: InteractionState,
): InteractionState {
  if (
    !next?.snapshot ||
    !Array.isArray(next.snapshot.requests) ||
    !Number.isSafeInteger(next.snapshot.revision)
  )
    return current;
  if (current.retiredEpochs?.includes(next.snapshot.epoch)) return current;
  if (current.snapshot.epoch && !next.snapshot.epoch) return current;
  if (
    current.snapshot.epoch === next.snapshot.epoch &&
    next.snapshot.revision < current.snapshot.revision
  )
    return current;
  const retired = current.retiredEpochs ?? [];
  return {
    ...next,
    retiredEpochs:
      current.snapshot.epoch && current.snapshot.epoch !== next.snapshot.epoch
        ? [...retired, current.snapshot.epoch]
        : retired,
  };
}
export function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}
export function wizardSteps(request: PendingInteraction) {
  const operations = Array.isArray(request.operations)
    ? request.operations
    : [];
  for (const operation of operations) {
    const components = asRecord(
      asRecord(operation).updateComponents,
    ).components;
    if (!Array.isArray(components)) continue;
    for (const component of components) {
      const value = asRecord(component);
      if (value.component === "ClarifyWizard" && value.variant !== "approval")
        return parseClarifySteps(value.steps);
    }
  }
  return [];
}
export function simpleSchema(request: PendingInteraction) {
  const schema = asRecord(request.responseSchema),
    properties = asRecord(schema.properties);
  if (
    schema.type !== "object" ||
    !Object.keys(properties).length ||
    schema.oneOf ||
    schema.anyOf ||
    schema.allOf
  )
    return null;
  if (
    Object.values(properties).some((v) => {
      const p = asRecord(v);
      return (
        !["string", "number", "integer", "boolean"].includes(String(p.type)) ||
        !!p.oneOf ||
        !!p.anyOf ||
        !!p.allOf ||
        !!p.pattern ||
        !!p.format ||
        !!p.$ref
      );
    })
  )
    return null;
  return {
    properties,
    required: new Set(
      Array.isArray(schema.required) ? (schema.required as string[]) : [],
    ),
  };
}
export function inlineInteraction(request: PendingInteraction) {
  return request.kind === "approval"
    ? request.actions.length > 0
    : request.kind === "question" &&
        (wizardSteps(request).length > 0 || !!simpleSchema(request));
}
export function redactInteractionDisplay(value: string) {
  return value
    .replace(/\bBearer\s+[^\s"']+/gi, "Bearer [已隐藏]")
    .replace(/\bsk-[A-Za-z0-9_-]{12,}/g, "[已隐藏]")
    .replace(
      /((?:api[_-]?key|access[_-]?token|password|secret)\s*[=:]\s*)(["']?)[^\s"',;]+\2/gi,
      "$1[已隐藏]",
    );
}
export function approvalDetails(request: PendingInteraction): string {
  const parts = [request.message];
  for (const operation of Array.isArray(request.operations)
    ? request.operations
    : []) {
    const components = asRecord(
      asRecord(operation).updateComponents,
    ).components;
    if (!Array.isArray(components)) continue;
    for (const c of components)
      for (const key of [
        "body",
        "approvalDetail",
        "approvalTypeLabel",
        "text",
      ]) {
        const text = asRecord(c)[key];
        if (typeof text === "string" && !parts.includes(text)) parts.push(text);
      }
  }
  return redactInteractionDisplay(parts.join("\n\n"));
}

export function taskRows(
  tasks: InteractionTask[],
): Array<{ task: InteractionTask; depth: number }> {
  const rows: Array<{ task: InteractionTask; depth: number }> = [],
    seen = new Set<string>();
  const visit = (task: InteractionTask, depth: number) => {
    if (seen.has(task.sessionId)) return;
    seen.add(task.sessionId);
    rows.push({ task, depth });
    tasks
      .filter((child) => child.parentSessionId === task.sessionId)
      .forEach((child) => visit(child, depth + 1));
  };
  tasks
    .filter(
      (task) =>
        !task.parentSessionId ||
        !tasks.some((p) => p.sessionId === task.parentSessionId),
    )
    .forEach((task) => visit(task, 0));
  tasks.forEach((task) => visit(task, 0)); // malformed/cyclic parents never hide a task
  return rows;
}
