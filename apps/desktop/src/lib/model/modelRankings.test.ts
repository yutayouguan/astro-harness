import assert from "node:assert/strict";
import test from "node:test";
import {
  appFaviconUrl,
  isRankingsEnvelopeFresh,
  normalizeApps,
  normalizeBenchmarks,
  normalizeSessionCosts,
  normalizeTasks,
  normalizeUsageRankings,
  type OpenRouterRankingsEnvelope,
} from "./modelRankings.ts";

function envelope(
  payload: unknown,
  overrides: Partial<OpenRouterRankingsEnvelope> = {},
): OpenRouterRankingsEnvelope {
  return {
    dataset: "text",
    modality: null,
    dataSource: "official",
    freshness: "fresh",
    cacheHit: false,
    usedFallback: false,
    fetchedAt: "2026-09-04T00:00:00Z",
    asOf: "2026-09-03T00:00:00Z",
    payload,
    ...overrides,
  };
}

test("normalizes official daily token rows into a weekly leaderboard", () => {
  const view = normalizeUsageRankings(
    envelope({
      data: [
        { date: "2026-09-02", model_permaslug: "a/model", total_tokens: "10" },
        { date: "2026-09-03", model_permaslug: "a/model", total_tokens: "15" },
        { date: "2026-09-03", model_permaslug: "b/model", total_tokens: "20" },
        { date: "2026-09-03", model_permaslug: "other", total_tokens: "999" },
      ],
    }),
  );

  assert.equal(view.unit, "tokens");
  assert.deepEqual(view.ranking, [
    { id: "a/model", value: 25, change: null },
    { id: "b/model", value: 20, change: null },
  ]);
  assert.equal(view.points.length, 2);
});

test("normalizes frontend modality chart data", () => {
  const view = normalizeUsageRankings(
    envelope(
      {
        data: {
          data: [
            { x: "2026-08-25", ys: { "a/embed": 5 } },
            { x: "2026-09-01", ys: { "a/embed": 7, "b/embed": 3 } },
          ],
        },
      },
      { dataset: "modality", modality: "embeddings", dataSource: "frontend" },
    ),
  );

  assert.equal(view.unit, "requests");
  assert.equal(view.ranking[0]?.id, "a/embed");
  assert.equal(view.ranking[0]?.value, 7);
});

test("normalizes official task classifications", () => {
  const view = normalizeTasks(
    envelope(
      {
        data: {
          window_days: 7,
          macro_categories: [
            { key: "code", label: "Code", usage_share: 0.4, token_share: 0.5 },
          ],
          classifications: [
            {
              tag: "code:general_impl",
              display_name: "Code Generation",
              macro_category: "code",
              usage_share: 0.2,
              token_share: 0.3,
              models: [{ id: "a/model", tag_token_share: 0.7 }],
            },
          ],
        },
      },
      { dataset: "tasks" },
    ),
  );

  assert.equal(view.windowDays, 7);
  assert.equal(view.categories[0]?.share, 0.5);
  assert.equal(view.tasks[0]?.models[0]?.id, "a/model");
});

test("normalizes frontend benchmark groups", () => {
  const rows = normalizeBenchmarks(
    envelope(
      {
        data: {
          aaData: {
            coding: [
              {
                heuristic_openrouter_slug: "a/model",
                aa_name: "Model A",
                score: 72.5,
              },
            ],
          },
        },
      },
      { dataset: "benchmarks", dataSource: "frontend" },
    ),
    "coding",
  );

  assert.deepEqual(rows, [{ id: "a/model", name: "Model A", score: 72.5 }]);
});

test("normalizes public app rankings", () => {
  const rows = normalizeApps(
    envelope(
      {
        data: {
          week: [
            {
              app_id: 7,
              total_tokens: "42",
              total_requests: 3,
              rank: 1,
              app: { slug: "astro", title: "Astro", categories: ["agent"] },
            },
          ],
        },
      },
      { dataset: "apps", dataSource: "frontend" },
    ),
  );

  assert.equal(rows[0]?.name, "Astro");
  assert.equal(rows[0]?.tokens, 42);
});

test("app icons use a safe favicon resolver", () => {
  const url = appFaviconUrl("https://nousresearch.com/");
  assert.ok(url?.startsWith("https://t0.gstatic.com/faviconV2?"));
  assert.ok(url?.includes("nousresearch.com"));
  assert.equal(appFaviconUrl("javascript:alert(1)"), null);
  assert.equal(appFaviconUrl("data:image/svg+xml,<svg/>"), null);
});

test("normalizes frontend task spend and model deltas", () => {
  const view = normalizeTasks(
    envelope(
      {
        data: {
          tokens: {
            windowDays: 30,
            macroCategories: [
              { key: "agent", label: "Agent", spendShare: 0.6 },
            ],
            tasks: [
              {
                tag: "agent:workflow_execution",
                macroCategory: "agent",
                spendShareOfTotal: 0.4,
                models: [{ model: "a/model", share: 0.7, deltaPp: 2.5 }],
              },
            ],
          },
        },
      },
      { dataset: "tasks", dataSource: "frontend" },
    ),
  );

  assert.equal(view.windowDays, 30);
  assert.equal(view.tasks[0]?.models[0]?.change, 2.5);
});

test("uses the core bucket for session cost comparison", () => {
  const rows = normalizeSessionCosts(
    envelope(
      {
        data: {
          harnesses: [
            {
              label: "Agent A",
              models: [
                {
                  model: "a/model",
                  points: [
                    { bucket: "short", medianUsd: 0.1 },
                    { bucket: "core", medianUsd: 0.5 },
                  ],
                },
              ],
            },
          ],
        },
      },
      { dataset: "session_cost", dataSource: "frontend" },
    ),
  );

  assert.deepEqual(rows, [
    { id: "a/model", harness: "Agent A", medianUsd: 0.5 },
  ]);
});

test("uses source-specific memory cache TTLs", () => {
  const now = Date.parse("2026-09-04T00:10:00Z");
  assert.equal(
    isRankingsEnvelopeFresh(
      envelope({}, { fetchedAt: "2026-09-04T00:00:30Z" }),
      now,
    ),
    true,
  );
  assert.equal(
    isRankingsEnvelopeFresh(
      envelope(
        {},
        { dataSource: "frontend", fetchedAt: "2026-09-04T00:00:30Z" },
      ),
      now,
    ),
    false,
  );
});
