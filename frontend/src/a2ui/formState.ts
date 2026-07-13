import type { A2uiComponent } from "./types";

export function mergeActionContext(
  base: Record<string, unknown> | undefined,
  fieldValues: Record<string, unknown>,
): Record<string, unknown> {
  return { ...fieldValues, ...(base ?? {}) };
}

/** Whether a field value counts as filled for required checks. */
export function isFieldFilled(value: unknown): boolean {
  if (value === undefined || value === null) return false;
  if (typeof value === "string") return value.trim().length > 0;
  if (typeof value === "boolean") return value;
  return true;
}

/** Ids of components marked `required: true` that are still empty. */
export function missingRequiredFields(
  components: A2uiComponent[],
  fieldValues: Record<string, unknown>,
): string[] {
  return components
    .filter((c) => c.required === true)
    .filter((c) => !isFieldFilled(fieldValues[c.id] ?? c.value))
    .map((c) => c.id);
}
