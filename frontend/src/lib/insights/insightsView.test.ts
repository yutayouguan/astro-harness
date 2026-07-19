// frontend/src/lib/insightsView.test.ts
import assert from "node:assert/strict";
import test from "node:test";
import {
  aggregateByProvider,
  DEFAULT_INSIGHTS_VIEW,
  inferProvider,
  INSIGHTS_VIEW_ORDER,
  needsUsageInsights,
  providerSpendTop,
  type InsightsRankItem,
} from "./insightsView.ts";

test("default view is overview and sits first in tab order", () => {
  assert.equal(DEFAULT_INSIGHTS_VIEW, "overview");
  assert.deepEqual(INSIGHTS_VIEW_ORDER, [
    "overview",
    "models",
    "tools",
    "collab",
    "tracing",
  ]);
});

test("needsUsageInsights covers overview, models, tools only", () => {
  assert.equal(needsUsageInsights("overview"), true);
  assert.equal(needsUsageInsights("models"), true);
  assert.equal(needsUsageInsights("tools"), true);
  assert.equal(needsUsageInsights("collab"), false);
  assert.equal(needsUsageInsights("tracing"), false);
});

test("providerSpendTop sorts by cost then tokens and caps length", () => {
  const items: InsightsRankItem[] = [
    { kind: "provider", name: "a", calls: 1, tokens: 10, cost_usd: 1 },
    { kind: "provider", name: "b", calls: 1, tokens: 90, cost_usd: 5 },
    { kind: "provider", name: "c", calls: 1, tokens: 50, cost_usd: 5 },
    { kind: "provider", name: "d", calls: 1, tokens: 1, cost_usd: 0.1 },
  ];
  const top = providerSpendTop(items, 2);
  assert.equal(top.length, 2);
  assert.equal(top[0]?.name, "b"); // cost tie → higher tokens first
  assert.equal(top[1]?.name, "c");
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
  assert.deepEqual(
    rows.map((r) => r.name).sort(),
    ["deepseek", "google"],
  );
  const google = rows.find((r) => r.name === "google");
  assert.equal(google?.calls, 2);
  assert.equal(google?.tokens, 100);
});
