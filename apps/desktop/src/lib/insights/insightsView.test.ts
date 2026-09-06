// apps/desktop/src/lib/insightsView.test.ts
import assert from "node:assert/strict";
import test from "node:test";
import {
  aggregateByProvider,
  costCoveragePercent,
  DEFAULT_INSIGHTS_VIEW,
  inferProvider,
  INSIGHTS_VIEW_ORDER,
  needsUsageInsights,
  providerDisplayName,
  rankByMetric,
  usageBucketState,
  type InsightsRankItem,
} from "./insightsView.ts";

test("default view is overview and sits first in tab order", () => {
  assert.equal(DEFAULT_INSIGHTS_VIEW, "overview");
  assert.deepEqual(INSIGHTS_VIEW_ORDER, [
    "overview",
    "models",
    "tools",
    "tracing",
  ]);
});

test("needsUsageInsights covers overview, models, tools only", () => {
  assert.equal(needsUsageInsights("overview"), true);
  assert.equal(needsUsageInsights("models"), true);
  assert.equal(needsUsageInsights("tools"), true);
  assert.equal(needsUsageInsights("tracing"), false);
});

test("rankByMetric keeps provider order aligned with the selected metric", () => {
  const items: InsightsRankItem[] = [
    { kind: "provider", name: "openai", calls: 3, tokens: 20, cost_usd: 1 },
    {
      kind: "provider",
      name: "deepseek",
      calls: 2,
      tokens: 90,
      cost_usd: 0.1,
    },
  ];
  assert.deepEqual(
    rankByMetric(items, "calls").map((item) => item.name),
    ["openai", "deepseek"],
  );
  assert.deepEqual(
    rankByMetric(items, "tokens").map((item) => item.name),
    ["deepseek", "openai"],
  );
  assert.deepEqual(
    rankByMetric(items, "cost").map((item) => item.name),
    ["openai", "deepseek"],
  );
});

test("costCoveragePercent distinguishes no data from partial coverage", () => {
  assert.equal(costCoveragePercent(0, 0), null);
  assert.equal(costCoveragePercent(10, 0), 100);
  assert.equal(costCoveragePercent(10, 3), 70);
  assert.equal(costCoveragePercent(10, 20), 0);
});

test("usageBucketState separates current, past, and future buckets", () => {
  const asOf = "2026-09-04T12:00:00.000Z";
  assert.equal(usageBucketState("2026-09-03", "month", asOf), "past");
  assert.equal(usageBucketState("2026-09-04", "month", asOf), "current");
  assert.equal(usageBucketState("2026-09-05", "month", asOf), "future");
  assert.equal(usageBucketState("2026-09-03", "days30", asOf), "past");
  assert.equal(usageBucketState("2026-09-04", "days90", asOf), "current");
  assert.equal(usageBucketState("2026-09-05", "days365", asOf), "future");
  assert.equal(usageBucketState("2026-08", "year", asOf), "past");
  assert.equal(usageBucketState("2026-09", "quarter", asOf), "current");
  assert.equal(usageBucketState("2026-10", "year", asOf), "future");
});

test("providerDisplayName normalizes brands and localizes fallbacks", () => {
  assert.equal(providerDisplayName("openai"), "OpenAI");
  assert.equal(providerDisplayName("deepseek"), "DeepSeek");
  assert.equal(providerDisplayName("other", "其他", "未知"), "其他");
  assert.equal(providerDisplayName("unknown", "其他", "未知"), "未知");
});

test("inferProvider maps Google models/ resource path to google", () => {
  assert.equal(inferProvider("models/gemini-3.5-flash"), "google");
  assert.equal(inferProvider("models/gemini-2.0-flash"), "google");
  assert.equal(inferProvider("gemini-2.5-pro"), "google");
});

test("inferProvider keeps OpenRouter vendor/model and bare prefixes", () => {
  assert.equal(inferProvider("deepseek/deepseek-chat"), "deepseek");
  assert.equal(inferProvider("openai/gpt-4o"), "openai");
  assert.equal(inferProvider("gpt-4o-mini"), "openai");
  assert.equal(inferProvider("claude-sonnet-4"), "anthropic");
});

test("aggregateByProvider does not create a fake models vendor", () => {
  const rows = aggregateByProvider([
    {
      kind: "llm",
      name: "models/gemini-3.5-flash",
      calls: 2,
      tokens: 100,
      cost_usd: 0.01,
    },
    {
      kind: "llm",
      name: "deepseek/deepseek-chat",
      calls: 1,
      tokens: 50,
      cost_usd: 0.02,
    },
  ]);
  assert.deepEqual(rows.map((r) => r.name).sort(), ["deepseek", "google"]);
  const google = rows.find((r) => r.name === "google");
  assert.equal(google?.calls, 2);
  assert.equal(google?.tokens, 100);
});
