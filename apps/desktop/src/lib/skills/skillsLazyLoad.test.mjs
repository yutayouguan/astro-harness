import assert from "node:assert/strict";
import test from "node:test";
import { isNearScrollEnd, pageHasMore } from "./skillsLazyLoad.ts";

test("pageHasMore stops when append adds nothing new", () => {
  assert.equal(pageHasMore(24, 24, 0), false);
  assert.equal(pageHasMore(24, 24, 12), true);
  assert.equal(pageHasMore(10, 24, 10), false);
});

test("near-bottom scroll metrics trigger dynamic loading", () => {
  assert.equal(
    isNearScrollEnd({ scrollTop: 640, scrollHeight: 1000, clientHeight: 240 }),
    true,
  );
  assert.equal(
    isNearScrollEnd({ scrollTop: 400, scrollHeight: 1000, clientHeight: 240 }),
    false,
  );
  assert.equal(
    isNearScrollEnd({ scrollTop: 0, scrollHeight: 0, clientHeight: 0 }),
    false,
  );
});

test("store cache key normalizes query and isolates marketplace filters", async () => {
  const {
    storeCacheKey,
    isStoreCacheFresh,
    STORE_CACHE_TTL_MS,
    LOCAL_SKILLS_TTL_MS,
  } = await import("./skillsLazyLoad.ts");
  assert.equal(storeCacheKey("  Weather "), storeCacheKey("weather"));
  assert.notEqual(
    storeCacheKey("weather", "all"),
    storeCacheKey("weather", "trending"),
  );
  assert.notEqual(
    storeCacheKey("weather", "all", "dev-programming"),
    storeCacheKey("weather", "all", "data-analysis"),
  );
  assert.equal(isStoreCacheFresh(Date.now() - 1000), true);
  assert.equal(isStoreCacheFresh(Date.now() - STORE_CACHE_TTL_MS - 1), false);
  assert.equal(
    isStoreCacheFresh(Date.now() - 30_000, Date.now(), LOCAL_SKILLS_TTL_MS),
    true,
  );
  assert.equal(
    isStoreCacheFresh(
      Date.now() - LOCAL_SKILLS_TTL_MS - 1,
      Date.now(),
      LOCAL_SKILLS_TTL_MS,
    ),
    false,
  );
});
