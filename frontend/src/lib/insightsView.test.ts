// frontend/src/lib/insightsView.test.ts
import assert from "node:assert/strict";
import test from "node:test";
import {
  DEFAULT_INSIGHTS_VIEW,
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
