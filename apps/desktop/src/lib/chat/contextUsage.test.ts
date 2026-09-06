import assert from "node:assert/strict";
import { test } from "node:test";
import {
  displayContextWindow,
  cacheHitPercent,
  formatTokenCount,
  normalizeContextUsageEvent,
  resolveContextWindow,
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

test("usagePercent reports real percent up to 100", () => {
  assert.equal(usagePercent(0, 128_000), 0);
  assert.equal(usagePercent(23_040, 128_000), 18);
  assert.equal(usagePercent(128_000, 128_000), 100);
  assert.equal(usagePercent(64_000, 1_000_000), 6);
  assert.equal(usagePercent(50_000, 0), 0);
});

test("resolveContextWindow prefers snapshot over model and never invents 128K", () => {
  assert.equal(resolveContextWindow(128_000, 1_048_576), 1_048_576);
  assert.equal(resolveContextWindow(1_000_000, null), 1_000_000);
  assert.equal(resolveContextWindow(null, 200_000), 200_000);
  assert.equal(resolveContextWindow(null, null), 0);
  assert.equal(resolveContextWindow(0, 0), 0);
});

test("displayContextWindow uses snapshot window first", () => {
  assert.equal(
    displayContextWindow({ contextWindow: 1_048_576 }, 128_000),
    1_048_576,
  );
  assert.equal(displayContextWindow(null, 200_000), 200_000);
  assert.equal(displayContextWindow(null, null), 0);
});

test("visibleSegments drops zeros and sorts by tokens desc", () => {
  const snap: ContextUsageSnapshot = {
    contextWindow: 128_000,
    totalTokens: 30,
    estimatedTotalTokens: 30,
    source: "local_estimate",
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
      context_window: 1_048_576,
      total_tokens: 42,
      estimated_total_tokens: 40,
      source: "provider_reported",
      latest_usage: {
        input_tokens: 40,
        uncached_input_tokens: 8,
        output_tokens: 2,
        total_tokens: 42,
        provider_total_tokens: 42,
        cache_read_tokens: 32,
        cache_read_reported: true,
        reasoning_tokens: 0,
        reasoning_reported: true,
      },
      updated_at: 1_700_000_000_000,
      recommend_compact: true,
      segments: [
        { id: "system", tokens: 10, count: 1 },
        { id: "tools", tokens: 0, count: null },
        {
          id: "skills",
          tokens: 32,
          items: [
            { id: "demo", label: "demo", tokens: 20 },
            { id: "skip", label: "skip", tokens: 0 },
          ],
        },
      ],
    }),
    {
      contextWindow: 1_048_576,
      totalTokens: 42,
      estimatedTotalTokens: 40,
      source: "provider_reported",
      latestUsage: {
        inputTokens: 40,
        uncachedInputTokens: 8,
        outputTokens: 2,
        totalTokens: 42,
        providerTotalTokens: 42,
        cacheReadTokens: 32,
        cacheWriteTokens: 0,
        reasoningTokens: 0,
        cacheReadReported: true,
        cacheWriteReported: false,
        reasoningReported: true,
      },
      updatedAt: 1_700_000_000_000,
      recommendCompact: true,
      segments: [
        { id: "system", tokens: 10, count: 1 },
        { id: "tools", tokens: 0 },
        {
          id: "skills",
          tokens: 32,
          items: [{ id: "demo", label: "demo", tokens: 20 }],
        },
      ],
    },
  );
});

test("cacheHitPercent distinguishes unsupported from an explicit zero hit", () => {
  const usage = {
    inputTokens: 100,
    uncachedInputTokens: 100,
    outputTokens: 10,
    totalTokens: 110,
    cacheReadTokens: 0,
    cacheWriteTokens: 0,
    reasoningTokens: 0,
    cacheReadReported: false,
    cacheWriteReported: false,
    reasoningReported: false,
  };
  assert.equal(cacheHitPercent(usage), null);
  assert.equal(cacheHitPercent({ ...usage, cacheReadReported: true }), 0);
  assert.equal(
    cacheHitPercent({
      ...usage,
      inputTokens: 100,
      cacheReadTokens: 75,
      cacheReadReported: true,
    }),
    75,
  );
});

test("SEGMENT_ORDER lists all known segments", () => {
  assert.deepEqual(SEGMENT_ORDER, [
    "system",
    "developer",
    "user_context",
    "tools",
    "agents",
    "mcp",
    "memory",
    "skills",
    "recall",
    "subagent",
    "conversation",
  ]);
});
