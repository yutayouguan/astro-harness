import assert from "node:assert/strict";
import test from "node:test";
import { createObservedTaskHistory } from "./observedTaskHistory.ts";

test("a late history read cannot overwrite the next conversation", async () => {
  let generation = 1;
  let finish!: (value: string) => void;
  const commits: string[] = [];
  const reader = createObservedTaskHistory({
    load: () =>
      new Promise<string>((resolve) => {
        finish = resolve;
      }),
    isCurrent: () => generation === 1,
    commit: (value) => commits.push(value),
  });
  const read = reader.refresh();
  generation = 2;
  finish("old conversation");
  await read;
  assert.deepEqual(commits, []);
});

test("history reads are serialized, retry failures and stop after disposal", async () => {
  let loads = 0;
  let finish!: (value: string) => void;
  const commits: string[] = [];
  const reader = createObservedTaskHistory({
    load: async () => {
      loads++;
      if (loads === 1) throw new Error("disconnected");
      return new Promise<string>((resolve) => {
        finish = resolve;
      });
    },
    isCurrent: () => true,
    commit: (value) => commits.push(value),
  });
  await reader.refresh();
  const read = reader.refresh();
  await reader.refresh();
  assert.equal(loads, 2);
  finish("latest persisted progress");
  await read;
  assert.deepEqual(commits, ["latest persisted progress"]);
  const late = reader.refresh();
  reader.dispose();
  finish("must not replace current view");
  await late;
  await reader.refresh();
  assert.equal(loads, 3);
  assert.equal(commits.length, 1);
});
