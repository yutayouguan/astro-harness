import type { PendingInterrupt } from "../../types";

function asRecord(value: unknown): Record<string, unknown> | undefined {
  return value != null && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined;
}

function coerceSchemaValue(value: unknown, schema: unknown): unknown {
  const type = asRecord(schema)?.type;
  if (typeof value !== "string") return value;
  if (type === "integer") {
    const parsed = Number(value);
    return Number.isInteger(parsed) ? parsed : value;
  }
  if (type === "number") {
    const parsed = Number(value);
    return Number.isFinite(parsed) ? parsed : value;
  }
  if (type === "boolean") {
    if (value.toLowerCase() === "true") return true;
    if (value.toLowerCase() === "false") return false;
  }
  return value;
}

export type ElicitationAction = "accept" | "decline" | "cancel";

export function resolveElicitationAction(
  actionName: string,
): ElicitationAction {
  if (actionName === "cancel") return "cancel";
  if (actionName === "deny") return "decline";
  return "accept";
}

/** Convert ClarifyWizard output back into the MCP elicitation schema object. */
export function buildElicitationContent(
  interrupt: PendingInterrupt,
  payload: Record<string, unknown>,
): Record<string, unknown> {
  const schema = asRecord(interrupt.responseSchema);
  const properties = asRecord(schema?.properties) ?? {};
  const propertyNames = Object.keys(properties);
  const answers = asRecord(payload.answers);
  const raw = answers
    ? answers
    : propertyNames.length === 1 && payload.value !== undefined
      ? { [propertyNames[0]]: payload.value }
      : payload;
  return Object.fromEntries(
    Object.entries(raw)
      .filter(([key]) => key !== "value" && key !== "approved")
      .map(([key, value]) => [key, coerceSchemaValue(value, properties[key])]),
  );
}

export function elicitationRequestId(interrupt: PendingInterrupt): string {
  const payload = asRecord(interrupt.metadata?.payload);
  return typeof payload?.mcp_request_id === "string"
    ? payload.mcp_request_id
    : interrupt.id;
}
