import assert from "node:assert/strict";
import test from "node:test";
import { petPixelAt, startPetHitTesting } from "./desktopPetHitTest.ts";

test("hit testing maps CSS pixels to Retina bitmap coordinates and excludes blank bounds", () => {
  const rect = { left: 20, top: 30, width: 192, height: 208 };
  assert.deepEqual(petPixelAt(116, 134, rect, 384, 416), { x: 192, y: 208 });
  assert.equal(petPixelAt(19, 134, rect, 384, 416), null);
  assert.equal(petPixelAt(212, 134, rect, 384, 416), null);
  assert.equal(petPixelAt(116, 238, rect, 384, 416), null);
});
test("late probe cannot re-enable click-through after disposal", async () => {
  let resolve!: (point: readonly [number, number]) => void;
  const writes: boolean[] = [];
  let scheduled = false;
  const stop = startPetHitTesting({
    probe: () =>
      new Promise((done) => {
        resolve = done;
      }),
    hit: () => false,
    setInteractive: async (value) => {
      writes.push(value);
    },
    schedule: () => {
      scheduled = true;
      return () => {};
    },
  });
  stop();
  resolve([0, 0]);
  await new Promise((done) => setImmediate(done));
  assert.deepEqual(writes, [true]);
  assert.equal(scheduled, false);
});
test("probe failures restore interactions instead of trapping an ignored window", async () => {
  const writes: boolean[] = [];
  const stop = startPetHitTesting({
    probe: async () => {
      throw new Error("native unavailable");
    },
    hit: () => false,
    setInteractive: async (value) => {
      writes.push(value);
    },
    schedule: () => () => {},
  });
  await new Promise((done) => setImmediate(done));
  assert.deepEqual(writes, [true]);
  stop();
});

test("global probing restores pet clicks after entering from transparent space", async () => {
  let opaque = false;
  let tick = () => {};
  const writes: boolean[] = [];
  const stop = startPetHitTesting({
    probe: async () => [50, 50],
    hit: () => opaque,
    setInteractive: async (value) => {
      writes.push(value);
    },
    schedule: (next) => {
      tick = next;
      return () => {};
    },
  });
  const flush = () => new Promise((done) => setImmediate(done));
  await flush();
  tick();
  await flush();
  assert.deepEqual(writes, [false]);
  opaque = true;
  tick();
  await flush();
  assert.deepEqual(writes, [false, true]);
  stop();
});
