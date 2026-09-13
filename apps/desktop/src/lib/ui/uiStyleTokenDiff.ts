/** Apply only changed tokens; wallpaper revisions must not clear the theme. */
export function applyUiStyleTokenDiff(
  style: Pick<
    CSSStyleDeclaration,
    "getPropertyValue" | "setProperty" | "removeProperty"
  >,
  previous: readonly string[],
  next: Record<string, string>,
) {
  for (const key of previous) if (!(key in next)) style.removeProperty(key);
  for (const [key, value] of Object.entries(next)) {
    if (style.getPropertyValue(key) !== value) style.setProperty(key, value);
  }
  return Object.keys(next);
}
