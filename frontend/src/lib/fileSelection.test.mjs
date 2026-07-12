import assert from "node:assert/strict";
import test from "node:test";
import {
  applyClick,
  applyToggle,
  applyRange,
  emptySelection,
} from "./fileSelection.ts";

const ids = ["a", "b", "c", "d"];

test("plain click replaces selection", () => {
  let s = emptySelection();
  s = applyClick(s, "b", ids);
  assert.deepEqual([...s.selectedIds], ["b"]);
  assert.equal(s.anchorId, "b");
});

test("toggle adds and removes", () => {
  let s = emptySelection();
  s = applyToggle(s, "a", ids);
  s = applyToggle(s, "c", ids);
  assert.deepEqual([...s.selectedIds].sort(), ["a", "c"]);
  s = applyToggle(s, "a", ids);
  assert.deepEqual([...s.selectedIds], ["c"]);
});

test("shift range from anchor", () => {
  let s = applyClick(emptySelection(), "b", ids);
  s = applyRange(s, "d", ids);
  assert.deepEqual([...s.selectedIds], ["b", "c", "d"]);
});
