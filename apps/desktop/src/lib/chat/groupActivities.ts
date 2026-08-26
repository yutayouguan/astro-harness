import type { ChatActivity } from "../../types";

export type ActivityBearingItem = {
  key: string;
  activity?: ChatActivity;
};

export type ConsecutiveActivityGroup<T extends ActivityBearingItem> = {
  type: "activity-group";
  key: string;
  items: T[];
};

export type GroupedActivityItem<T extends ActivityBearingItem> =
  | T
  | ConsecutiveActivityGroup<T>;

/** Group only adjacent activity items; narrative/surface items remain hard boundaries. */
export function groupConsecutiveActivities<T extends ActivityBearingItem>(
  items: T[],
): GroupedActivityItem<T>[] {
  const result: GroupedActivityItem<T>[] = [];
  let run: T[] = [];

  const flush = () => {
    if (run.length === 1) result.push(run[0]!);
    if (run.length > 1) {
      result.push({
        type: "activity-group",
        key: `activity-group-${run[0]!.key}`,
        items: run,
      });
    }
    run = [];
  };

  for (const item of items) {
    if (item.activity) {
      run.push(item);
      continue;
    }
    flush();
    result.push(item);
  }
  flush();
  return result;
}

export function isConsecutiveActivityGroup<T extends ActivityBearingItem>(
  item: GroupedActivityItem<T>,
): item is ConsecutiveActivityGroup<T> {
  return "type" in item && item.type === "activity-group";
}
