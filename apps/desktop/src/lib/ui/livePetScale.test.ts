import assert from "node:assert/strict";
import test from "node:test";
import {
  createLivePetScaleQueue,
  petScaleGestureUndo,
  type LivePetScaleRequest,
} from "./livePetScale.ts";

test("live resize starts before release and flushes only the latest pending size", async () => {
  const calls: LivePetScaleRequest[] = [];
  let finish!: (ok: boolean) => void;
  const busy: boolean[] = [];
  const queue = createLivePetScaleQueue({
    apply: (request) => {
      calls.push(request);
      return calls.length === 1
        ? new Promise((resolve) => {
            finish = resolve;
          })
        : Promise.resolve(true);
    },
    busy: (value) => busy.push(value),
    settled: (ok) => assert.equal(ok, true),
  });
  const done = queue.enqueue({
    petId: "cat",
    scale: 0.25,
    gestureId: "drag-1",
  });
  assert.equal(calls.length, 1, "apply immediately, without pointerup");
  void queue.enqueue({ petId: "cat", scale: 0.22, gestureId: "drag-1" });
  void queue.enqueue({ petId: "cat", scale: 0.18, gestureId: "drag-1" });
  assert.equal(calls.length, 1, "one in-flight native request");
  finish(true);
  await done;
  assert.deepEqual(
    calls.map((call) => call.scale),
    [0.25, 0.18],
  );
  assert.deepEqual(busy, [true, false]);
});

test("failure clears queued resizes and allows retry", async () => {
  let finish!: (ok: boolean) => void;
  const results: boolean[] = [];
  let calls = 0;
  const queue = createLivePetScaleQueue({
    apply: () => {
      calls++;
      return new Promise((resolve) => {
        finish = resolve;
      });
    },
    busy: () => {},
    settled: (ok) => results.push(ok),
  });
  const done = queue.enqueue({ petId: "cat", scale: 0.25, gestureId: "a" });
  void queue.enqueue({ petId: "cat", scale: 0.2, gestureId: "a" });
  finish(false);
  await done;
  assert.equal(calls, 1);
  assert.equal(queue.isPending(), false);
  const retry = queue.enqueue({ petId: "cat", scale: 0.2, gestureId: "b" });
  finish(true);
  await retry;
  assert.deepEqual(results, [false, true]);
});

test("one drag retains its first undo size; external edits and new drags split history", () => {
  const first = petScaleGestureUndo(
    null,
    { petId: "cat", scale: 0.25, gestureId: "a" },
    { activePetId: "cat", scale: 0.3 },
    0.25,
  );
  const last = petScaleGestureUndo(
    first,
    { petId: "cat", scale: 0.15, gestureId: "a" },
    { activePetId: "cat", scale: 0.25 },
    0.15,
  );
  assert.equal(last.undo.before, 0.3);
  assert.equal(last.undo.expected, 0.15);
  const external = petScaleGestureUndo(
    first,
    { petId: "cat", scale: 0.15, gestureId: "a" },
    { activePetId: "cat", scale: 0.22 },
    0.15,
  );
  assert.equal(external.undo.before, 0.22);
  const next = petScaleGestureUndo(
    last,
    { petId: "cat", scale: 0.2, gestureId: "b" },
    { activePetId: "cat", scale: 0.15 },
    0.2,
  );
  assert.equal(next.undo.before, 0.15);
  const otherPet = petScaleGestureUndo(
    first,
    { petId: "dog", scale: 0.2, gestureId: "a" },
    { activePetId: "dog", scale: 0.3 },
    0.2,
  );
  assert.equal(otherPet.undo.before, 0.3);
});
