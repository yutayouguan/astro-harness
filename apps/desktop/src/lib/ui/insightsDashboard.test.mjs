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
const usageDashboard = await readFile(
  new URL("../../components/settings/UsageDashboard.tsx", import.meta.url),
  "utf8",
);
const usageStyles = await readFile(
  new URL("../../styles/features/usage-dashboard.css", import.meta.url),
  "utf8",
);

test("overview exposes the usage summary, cache ratio, and recent requests", () => {
  assert.match(source, /<UsageDashboard data=\{data\}/);
  assert.match(usageDashboard, /data\.kpis\.cost_usd/);
  assert.match(usageDashboard, /data\.kpis\.llm_calls/);
  assert.match(usageDashboard, /data\.kpis\.cache_read_tokens/);
  assert.match(usageDashboard, /data\.recent_requests/);
  assert.match(usageDashboard, /usage-request-table/);
});

test("overview analysis switches between token and cost charts", () => {
  assert.match(usageDashboard, /type Metric = "tokens" \| "cost"/);
  assert.match(usageDashboard, /setMetric\(value\)/);
  assert.match(usageDashboard, /setChartType\("bar"\)/);
  assert.match(usageDashboard, /setChartType\("line"\)/);
});

test("insights layout has responsive KPI grids and accessible fallbacks", () => {
  assert.match(
    usageStyles,
    /\.usage-kpi-grid\s*\{[\s\S]*?repeat\(4, minmax\(0, 1fr\)\)/,
  );
  assert.match(usageStyles, /@media \(max-width: 980px\)/);
  assert.match(usageStyles, /@media \(max-width: 680px\)/);
  assert.match(styles, /@media \(prefers-reduced-motion: reduce\)/);
  assert.match(styles, /@media \(prefers-reduced-transparency: reduce\)/);
  assert.match(styles, /@media \(prefers-contrast: more\)/);
});

test("insights selections and cards share one scoped material hierarchy", () => {
  assert.match(
    styles,
    /--insights-surface:\s*var\(\s*--settings-panel-background,\s*var\(--glass-fill\)\)/,
  );
  assert.match(
    styles,
    /\.insights-panel :is\(\.insights-view-tabs, \.insights-period-tabs\)/,
  );
  assert.match(
    styles,
    /:is\(\s*\.insights-view-tab,\s*\.insights-period-tab\s*\)\.ui-segmented-tabs__tab\.is-active/,
  );
  assert.match(
    styles,
    /\.insights-kpi\s*\{[\s\S]*?background: var\(--insights-surface\);[\s\S]*?box-shadow: var\(--insights-panel-shadow\)/,
  );
  assert.match(
    styles,
    /\.insights-trace-list-panel,\s*\.insights-trace-chain-panel\s*\{[\s\S]*?background: var\(--insights-surface\);[\s\S]*?box-shadow: var\(--insights-panel-shadow\)/,
  );
});

test("model, capability, and trace views expose distinctive summary cards", () => {
  assert.match(source, /insights-kpis insights-kpis-summary/);
  assert.match(source, /insights\.summary\.topModel/);
  assert.match(source, /capabilityCallTotal/);
  assert.match(source, /traceAgentActions/);
  assert.match(source, /data-accent=\{tone\}/);
  assert.match(styles, /\.insights-kpi\[data-accent="blue"\]/);
  assert.match(styles, /--insights-kpi-accent/);
  assert.match(styles, /\.insights-kpi-value\.is-text/);
  assert.match(
    styles,
    /@media \(max-width: 640px\)[\s\S]*?\.insights-kpis-summary[\s\S]*?grid-template-columns: 1fr/,
  );
});

test("future buckets and keyboard focus have distinct chart treatments", () => {
  assert.match(source, /usageBucketState/);
  assert.match(source, /tabIndex=\{bucketState === "future" \? -1 : 0\}/);
  assert.match(styles, /\.insights-bar-col\.is-future/);
  assert.match(styles, /\.insights-bar-col:focus-visible/);
  assert.match(styles, /\.insights-bar-tooltip/);
});
