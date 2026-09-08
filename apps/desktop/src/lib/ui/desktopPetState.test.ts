import assert from "node:assert/strict";
import test from "node:test";
import {
  acceptDesktopPetState,
  createPetMutationQueue,
  EMPTY_DESKTOP_PET_STATE,
  subscribeDesktopPetState,
} from "./desktopPetState.ts";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

test("a slow initial snapshot cannot replace a newer live state", async () => {
  const snapshot = deferred<typeof EMPTY_DESKTOP_PET_STATE>();
  const ready = deferred<void>();
  let current = EMPTY_DESKTOP_PET_STATE;
  let receive!: (value: typeof current) => void;
  const dispose = subscribeDesktopPetState({
    listen: async (callback) => {
      receive = callback;
      return () => {};
    },
    snapshot: () => snapshot.promise,
    receive: (value) => {
      current = acceptDesktopPetState(current, value);
    },
    error: (cause) => {
      throw cause;
    },
    ready: () => ready.resolve(),
  });
  await Promise.resolve();
  receive({ ...current, revision: 2, enabled: true });
  snapshot.resolve({ ...EMPTY_DESKTOP_PET_STATE, revision: 1 });
  await ready.promise;
  assert.equal(current.enabled, true);
  assert.equal(current.revision, 2);
  dispose();
});

test("disposing before listener registration completes cleans it up without fetching", async () => {
  const registered = deferred<() => void>();
  let stopped = 0;
  let fetched = 0;
  const dispose = subscribeDesktopPetState({
    listen: () => registered.promise,
    snapshot: async () => {
      fetched++;
      return EMPTY_DESKTOP_PET_STATE;
    },
    receive: () => assert.fail("received after disposal"),
    error: () => assert.fail("error after disposal"),
    ready: () => {},
  });
  dispose();
  registered.resolve(() => stopped++);
  await registered.promise;
  assert.equal(stopped, 1);
  assert.equal(fetched, 0);
});

test("mutations preserve user order and continue after a failure", async () => {
  const queue = createPetMutationQueue();
  const first = deferred<number>();
  const order: number[] = [];
  const a = queue(async () => {
    order.push(1);
    return first.promise;
  });
  const b = queue(async () => {
    order.push(2);
    return 2;
  });
  await Promise.resolve();
  assert.deepEqual(order, [1]);
  first.reject(new Error("failed first write"));
  await assert.rejects(a);
  assert.equal(await b, 2);
  assert.deepEqual(order, [1, 2]);
});
