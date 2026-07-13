export function mergeActionContext(
  base: Record<string, unknown> | undefined,
  fieldValues: Record<string, unknown>,
): Record<string, unknown> {
  return { ...fieldValues, ...(base ?? {}) };
}
