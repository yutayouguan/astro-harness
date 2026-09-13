import test from "node:test";
import assert from "node:assert/strict";
import { createPetAssetCache } from "./petAssetCache.ts";

test("cache deduplicates active loads and evicts only released least-recent entries", async () => {
  let loads = 0;
  const cache = createPetAssetCache(20, async (key, _, reserve) => {
    loads++;
    reserve(10);
    return key;
  });
  const a = cache.acquire("a"),
    same = cache.acquire("a");
  assert.equal(await a.ready, "a");
  await same.ready;
  assert.equal(loads, 1);
  const b = cache.acquire("b");
  await b.ready;
  const full = cache.acquire("c");
  await assert.rejects(full.ready, /budget/);
  full.release();
  a.release();
  same.release();
  const c = cache.acquire("c");
  await c.ready;
  assert.deepEqual(cache.stats(), { bytes: 20, entries: 2 });
  b.release();
  c.release();
  c.release();
});

test("cancelled decode stays charged until settled, and cannot delete a newer same-key load", async () => {
  const finish: (() => void)[] = [];
  const cache = createPetAssetCache(20, async (key, _, reserve) => {
    reserve(10);
    await new Promise<void>((resolve) => finish.push(resolve));
    return key;
  });
  const old = cache.acquire("a");
  await Promise.resolve();
  old.release();
  assert.equal(cache.stats().bytes, 10);
  const next = cache.acquire("a");
  await Promise.resolve();
  finish[0]();
  await assert.rejects(old.ready, /cancelled/);
  assert.equal(cache.stats().bytes, 10);
  finish[1]();
  assert.equal(await next.ready, "a");
  assert.deepEqual(cache.stats(), { bytes: 10, entries: 1 });
  next.release();
});
