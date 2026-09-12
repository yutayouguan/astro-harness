import { parseClarifySteps } from "../../a2ui/clarifySteps.ts";
import type { PendingInterrupt } from "../../types.ts";
import {
  buildElicitationContent,
  resolveElicitationAction,
} from "./elicitation.ts";
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
  expanded: boolean;
  uiRevision: number;
  connected: boolean;
  snapshot: InteractionSnapshot;
  selected: string | null;
  retiredEpochs?: string[];
};
export const EMPTY_INTERACTIONS: InteractionState = {
  expanded: false,
  uiRevision: 0,
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
    (next.snapshot.revision < current.snapshot.revision ||
      next.uiRevision < current.uiRevision)
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

/** Raw MCP IDs can collide across servers or with a HITL UUID. */
export function interactionIdentity(request: PendingInteraction): string {
  return request.serverName
    ? `mcp:${request.toolCallId}`
    : `hitl:${request.requestId}`;
}
export function interruptIdentity(interrupt: PendingInterrupt): string {
  if (interrupt.reason === "elicitation") {
    const metadata = asRecord(interrupt.metadata?.payload);
    const callId =
      interrupt.toolCallId ??
      (typeof metadata.server_name === "string"
        ? `mcp-elicitation:${metadata.server_name}:${metadata.mcp_request_id ?? interrupt.id}`
        : "");
    return `mcp:${callId}`;
  }
  return `hitl:${interrupt.id}`;
}
export function interactionMatchesInterrupt(
  request: PendingInteraction,
  sessionId: string | null,
  interrupt: PendingInterrupt,
) {
  return (
    request.sessionId === sessionId &&
    interactionIdentity(request) === interruptIdentity(interrupt)
  );
}

/** Normalize a legacy A2UI action before entering the request-level controller. */
export function legacyInteractionResponse(
  request: PendingInteraction,
  name: string,
  context: Record<string, unknown>,
) {
  if (request.kind === "approval") {
    const option = request.actions.find((action) => action.id === name);
    if (!option) throw new Error("请使用当前审批卡片提供的授权选项");
    return { action: option.id, payload: {}, persistent: option.persistent };
  }
  if (request.serverName) {
    const action =
      name === "decline" ? "decline" : resolveElicitationAction(name);
    return {
      action: action === "accept" ? "submit" : action,
      payload:
        action === "accept"
          ? buildElicitationContent(
              {
                id: request.requestId,
                reason: "elicitation",
                responseSchema: request.responseSchema,
              },
              context,
            )
          : {},
      persistent: false,
    };
  }
  return { action: "submit", payload: context, persistent: false };
}
export function redactInteractionDisplay(value: string) {
  return value
    .replace(/\bBearer\s+[^\s"']+/gi, "Bearer [已隐藏]")
    .replace(/\bsk-[A-Za-z0-9_-]{12,}/g, "[已隐藏]")
    .replace(
      /((?:api[_-]?key|access[_-]?token|password|secret)["']?\s*[=:]\s*)(?:"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|[^\s"',;]+)/gi,
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
