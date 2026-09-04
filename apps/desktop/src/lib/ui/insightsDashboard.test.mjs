import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const source = await readFile(
  new URL("../../components/settings/InsightsPanel.tsx", import.meta.url),
  "utf8",
);
const styles = await readFile(
  new URL("../../styles/features/insights.css", import.meta.url),
  "utf8",
);

test("overview exposes pricing coverage and the full token breakdown", () => {
  assert.match(source, /costCoveragePercent/);
  assert.match(source, /formatEstimatedCost/);
  assert.match(source, /insights\.kpi\.unpriced/);
  assert.match(source, /insights\.kpi\.inputTokens/);
  assert.match(source, /insights\.kpi\.outputTokens/);
  assert.match(source, /insights\.kpi\.cacheTokens/);
  assert.match(source, /insights\.kpi\.reasoningTokens/);
  assert.match(source, /insights\.kpi\.averageTokens/);
});

test("provider ranking follows the selected metric", () => {
  assert.match(source, /rankByMetric\(byProvider, metric, 5\)/);
  assert.match(source, /providerRankTitleKey\(metric\)/);
  assert.match(source, /rankValue\(r, metric\)/);
});

test("insights layout has responsive KPI grids and accessible fallbacks", () => {
  assert.match(
    styles,
    /\.insights-kpis-overview\s*\{[\s\S]*?repeat\(4, minmax\(0, 1fr\)\)/,
  );
  assert.match(
    styles,
    /\.insights-kpis-breakdown\s*\{[\s\S]*?repeat\(5, minmax\(0, 1fr\)\)/,
  );
  assert.match(styles, /@media \(prefers-reduced-motion: reduce\)/);
  assert.match(styles, /@media \(prefers-reduced-transparency: reduce\)/);
  assert.match(styles, /@media \(prefers-contrast: more\)/);
});

test("future buckets and keyboard focus have distinct chart treatments", () => {
  assert.match(source, /usageBucketState/);
  assert.match(source, /tabIndex=\{bucketState === "future" \? -1 : 0\}/);
  assert.match(styles, /\.insights-bar-col\.is-future/);
  assert.match(styles, /\.insights-bar-col:focus-visible/);
  assert.match(styles, /\.insights-bar-tooltip/);
});
