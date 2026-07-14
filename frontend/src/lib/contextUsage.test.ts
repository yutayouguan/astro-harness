import assert from "node:assert/strict";
import { test } from "node:test";
import {
  formatTokenCount,
  normalizeContextUsageEvent,
  usagePercent,
  visibleSegments,
  SEGMENT_ORDER,
  type ContextUsageSnapshot,
} from "./contextUsage.ts";

test("formatTokenCount", () => {
  assert.equal(formatTokenCount(498), "498");
  assert.equal(formatTokenCount(9800), "9.8K");
  assert.equal(formatTokenCount(128_000), "128K");
});

test("usagePercent caps at 99 for bar label", () => {
  assert.equal(usagePercent(0, 128_000), 0);
  assert.equal(usagePercent(23_040, 128_000), 18);
  assert.equal(usagePercent(128_000, 128_000), 99);
});

test("visibleSegments drops zeros and sorts by tokens desc", () => {
  const snap: ContextUsageSnapshot = {
    contextWindow: 128_000,
    totalTokens: 30,
    segments: [
      { id: "system", tokens: 10 },
      { id: "tools", tokens: 0 },
      { id: "conversation", tokens: 20 },
    ],
    updatedAt: 1,
  };
  assert.deepEqual(
    visibleSegments(snap).map((s) => s.id),
    ["conversation", "system"],
  );
});

test("normalizeContextUsageEvent maps snake_case Tauri payload", () => {
  assert.deepEqual(
    normalizeContextUsageEvent({
      context_window: 128_000,
      total_tokens: 42,
      updated_at: 1_700_000_000_000,
      segments: [
        { id: "system", tokens: 10, count: 1 },
        { id: "tools", tokens: 0, count: null },
        { id: "conversation", tokens: 32 },
      ],
    }),
    {
      contextWindow: 128_000,
      totalTokens: 42,
      updatedAt: 1_700_000_000_000,
      segments: [
        { id: "system", tokens: 10, count: 1 },
        { id: "tools", tokens: 0 },
        { id: "conversation", tokens: 32 },
      ],
    },
  );
});

test("SEGMENT_ORDER lists all known segments", () => {
  assert.equal(SEGMENT_ORDER.length, 8);
});
