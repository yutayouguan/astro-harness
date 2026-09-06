export type SegmentedTabNavigationKey =
  | "ArrowLeft"
  | "ArrowRight"
  | "Home"
  | "End";

export function nextEnabledTabIndex(
  currentIndex: number,
  key: SegmentedTabNavigationKey,
  disabled: readonly boolean[],
): number {
  const enabled = disabled
    .map((isDisabled, index) => (isDisabled ? -1 : index))
    .filter((index) => index >= 0);

  if (enabled.length === 0) return -1;
  if (key === "Home") return enabled[0] ?? -1;
  if (key === "End") return enabled[enabled.length - 1] ?? -1;

  const position = enabled.indexOf(currentIndex);
  const start = position >= 0 ? position : 0;
  const delta = key === "ArrowRight" ? 1 : -1;
  return enabled[(start + delta + enabled.length) % enabled.length] ?? -1;
}

export function isSegmentedTabNavigationKey(
  key: string,
): key is SegmentedTabNavigationKey {
  return ["ArrowLeft", "ArrowRight", "Home", "End"].includes(key);
}
