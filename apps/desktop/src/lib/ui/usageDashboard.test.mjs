import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const [panel, dashboard, styles, messages, commands, usageDb] =
  await Promise.all(
    [
      "../../components/settings/InsightsPanel.tsx",
      "../../components/settings/UsageDashboard.tsx",
      "../../styles/features/usage-dashboard.css",
      "../../i18n/messages.ts",
      "../../../src-tauri/src/commands/config.rs",
      "../../../../../crates/agent-usage/src/db.rs",
    ].map((path) => readFile(new URL(path, import.meta.url), "utf8")),
  );

test("usage settings expose rolling periods and a dedicated overview", () => {
  assert.match(messages, /"settings\.sidebar\.tab\.insights": "用量统计"/);
  assert.match(panel, /type Period = "days30" \| "days90" \| "days365"/);
  assert.match(
    panel,
    /<UsageDashboard data=\{data\} annualSeries=\{annualSeries\}/,
  );
  assert.doesNotMatch(panel, /insights-legacy-overview/);
});

test("usage dashboard includes KPIs, activity heatmap, analysis, and requests", () => {
  assert.match(dashboard, /usage-kpi-grid/);
  assert.match(dashboard, /usage-heatmap/);
  assert.match(dashboard, /usage-analysis-card/);
  assert.match(dashboard, /usage-request-table/);
  assert.match(dashboard, /recent_requests/);
  assert.match(styles, /grid-template-columns: repeat\(4, minmax\(0, 1fr\)\)/);
  assert.match(styles, /grid-template-rows: repeat\(7, 9px\)/);
});

test("wide ranges use adaptive buckets while the annual heatmap stays daily", () => {
  assert.match(panel, /period: "days365",\s*granularity: "day"/);
  assert.match(dashboard, /data\.granularity/);
  assert.match(dashboard, /shouldShowAxisLabel/);
  assert.match(dashboard, /formatBucketRange/);
  assert.match(dashboard, /usage-chart-empty-state/);
  assert.match(dashboard, /className="usage-bar-column"[\s\S]*?tabIndex=\{0\}/);
  assert.match(styles, /\.usage-chart-empty-state/);
  assert.match(commands, /pub granularity: Option<String>/);
  assert.match(usageDb, /pub enum UsageGranularity/);
  assert.match(usageDb, /UsagePeriod::Days90 => Self::Week/);
  assert.match(usageDb, /UsagePeriod::Days365[^\n]*UsagePeriod::Quarter/);
});

test("rolling periods and request rows are backed by the usage database", () => {
  for (const period of ["days30", "days90", "days365"]) {
    assert.match(commands, new RegExp(`"${period}" => usage::UsagePeriod::`));
  }
  assert.match(usageDb, /pub recent_requests: Vec<UsageRequestRow>/);
  assert.match(usageDb, /async fn query_recent_requests/);
  assert.match(usageDb, /pub cache_read_tokens: i64/);
  assert.match(usageDb, /pub cache_write_tokens: i64/);
});
