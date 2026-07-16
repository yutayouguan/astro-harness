import assert from "node:assert/strict";
import test from "node:test";
import {
  createLazyLoadGate,
  decideLazyLoad,
  pageHasMore,
} from "./skillsLazyLoad.ts";

test("rising edge triggers load once while sentinel stays visible", () => {
  let gate = createLazyLoadGate();

  let d = decideLazyLoad(gate, {
    isIntersecting: true,
    hasMore: true,
    isLoading: false,
  });
  assert.equal(d.shouldLoad, true);
  gate = d.next;

  // still intersecting after fetch — must NOT fire again
  d = decideLazyLoad(gate, {
    isIntersecting: true,
    hasMore: true,
    isLoading: false,
  });
  assert.equal(d.shouldLoad, false);
  gate = d.next;

  d = decideLazyLoad(gate, {
    isIntersecting: true,
    hasMore: true,
    isLoading: false,
  });
  assert.equal(d.shouldLoad, false);
});

test("leaving and re-entering sentinel arms the next page load", () => {
  let gate = createLazyLoadGate();

  let d = decideLazyLoad(gate, {
    isIntersecting: true,
    hasMore: true,
    isLoading: false,
  });
  assert.equal(d.shouldLoad, true);
  gate = d.next;

  d = decideLazyLoad(gate, {
    isIntersecting: false,
    hasMore: true,
    isLoading: false,
  });
  assert.equal(d.shouldLoad, false);
  gate = d.next;
  assert.equal(gate.wasIntersecting, false);

  d = decideLazyLoad(gate, {
    isIntersecting: true,
    hasMore: true,
    isLoading: false,
  });
  assert.equal(d.shouldLoad, true);
});

test("does not load when hasMore is false or already loading", () => {
  let gate = createLazyLoadGate();

  let d = decideLazyLoad(gate, {
    isIntersecting: true,
    hasMore: false,
    isLoading: false,
  });
  assert.equal(d.shouldLoad, false);

  gate = createLazyLoadGate();
  d = decideLazyLoad(gate, {
    isIntersecting: true,
    hasMore: true,
    isLoading: true,
  });
  assert.equal(d.shouldLoad, false);
});

test("suppressInitial skips first visible sentinel until user scrolls away", () => {
  let gate = createLazyLoadGate({ suppressInitial: true });

  let d = decideLazyLoad(gate, {
    isIntersecting: true,
    hasMore: true,
    isLoading: false,
  });
  assert.equal(d.shouldLoad, false);
  gate = d.next;

  d = decideLazyLoad(gate, {
    isIntersecting: false,
    hasMore: true,
    isLoading: false,
  });
  gate = d.next;

  d = decideLazyLoad(gate, {
    isIntersecting: true,
    hasMore: true,
    isLoading: false,
  });
  assert.equal(d.shouldLoad, true);
});

test("pageHasMore stops when append adds nothing new", () => {
  assert.equal(pageHasMore(24, 24, 0), false);
  assert.equal(pageHasMore(24, 24, 12), true);
  assert.equal(pageHasMore(10, 24, 10), false);
});

test("store cache key normalizes query", async () => {
  const { storeCacheKey, isStoreCacheFresh, STORE_CACHE_TTL_MS, LOCAL_SKILLS_TTL_MS } =
    await import("./skillsLazyLoad.ts");
  assert.equal(storeCacheKey("clawhub", "  Weather "), storeCacheKey("clawhub", "weather"));
  assert.equal(isStoreCacheFresh(Date.now() - 1000), true);
  assert.equal(
    isStoreCacheFresh(Date.now() - STORE_CACHE_TTL_MS - 1),
    false,
  );
  assert.equal(
    isStoreCacheFresh(Date.now() - 30_000, Date.now(), LOCAL_SKILLS_TTL_MS),
    true,
  );
  assert.equal(
    isStoreCacheFresh(Date.now() - LOCAL_SKILLS_TTL_MS - 1, Date.now(), LOCAL_SKILLS_TTL_MS),
    false,
  );
});
