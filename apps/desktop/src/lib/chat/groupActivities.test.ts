import { test } from "node:test";
import assert from "node:assert/strict";
import type { ChatActivity } from "../../types.ts";
import {
  groupConsecutiveActivities,
  isConsecutiveActivityGroup,
} from "./groupActivities.ts";

type Item = { key: string; activity?: ChatActivity };

function tool(key: string): Item {
  return { key, activity: { id: key, kind: "tool", title: key } };
}

test("groups adjacent activities but keeps narrative boundaries", () => {
  const grouped = groupConsecutiveActivities([
    { key: "reasoning" },
    tool("read"),
    tool("search"),
    { key: "reply" },
    tool("run"),
  ]);

  assert.equal(grouped.length, 4);
  assert.equal(grouped[0]?.key, "reasoning");
  assert.ok(grouped[1] && isConsecutiveActivityGroup(grouped[1]));
  if (grouped[1] && isConsecutiveActivityGroup(grouped[1])) {
    assert.deepEqual(grouped[1].items.map((item) => item.key), ["read", "search"]);
  }
  assert.equal(grouped[2]?.key, "reply");
  assert.equal(grouped[3]?.key, "run");
});

test("does not wrap a single activity in an extra group", () => {
  const item = tool("read");
  assert.deepEqual(groupConsecutiveActivities([item]), [item]);
});
