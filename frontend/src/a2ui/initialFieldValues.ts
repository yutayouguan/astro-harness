export function mergeInitialFieldValues(
  current: Record<string, unknown>,
  initial: Record<string, unknown>,
): Record<string, unknown> {
  const next = { ...current };
  for (const [key, value] of Object.entries(initial)) {
    const existing = next[key];
    const empty =
      existing == null || (typeof existing === "string" && !existing.trim());
    if (empty) next[key] = value;
  }
  return next;
}
