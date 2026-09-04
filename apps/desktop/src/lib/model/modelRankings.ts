export type RankingsDataSource = "official" | "frontend";
export type RankingsFreshness = "fresh" | "stale";

export interface OpenRouterRankingsEnvelope {
  dataset: string;
  modality: string | null;
  dataSource: RankingsDataSource;
  freshness: RankingsFreshness;
  cacheHit: boolean;
  usedFallback: boolean;
  fetchedAt: string;
  asOf: string | null;
  payload: unknown;
}

export function isRankingsEnvelopeFresh(
  envelope: OpenRouterRankingsEnvelope,
  now = Date.now(),
): boolean {
  const fetchedAt = new Date(envelope.fetchedAt).getTime();
  if (!Number.isFinite(fetchedAt)) return false;
  const ttlMs =
    envelope.dataSource === "official" ? 6 * 60 * 60_000 : 60 * 60_000;
  return now - fetchedAt <= ttlMs;
}

/**
 * Auto-load a ranking key at most once per mounted panel. Cached stale data is
 * still rendered immediately, then revalidated once in the background. Manual
 * refreshes intentionally bypass this decision.
 */
export function shouldAutoLoadRankings(
  envelope: OpenRouterRankingsEnvelope | null,
  attempted: boolean,
  now = Date.now(),
): boolean {
  return !attempted && (!envelope || !isRankingsEnvelopeFresh(envelope, now));
}

export interface RankingPoint {
  x: string;
  ys: Record<string, number>;
}

export interface RankingItem {
  id: string;
  label?: string;
  value: number;
  change: number | null;
}

export interface UsageRankingView {
  points: RankingPoint[];
  ranking: RankingItem[];
  unit: "tokens" | "requests" | "subrequests";
}

export interface TaskRanking {
  id: string;
  label: string;
  category: string;
  share: number;
  models: Array<{ id: string; share: number; change: number | null }>;
}

export interface TaskRankingsView {
  windowDays: number;
  categories: Array<{ id: string; label: string; share: number }>;
  tasks: TaskRanking[];
  metric: "spend" | "tokens";
}

export type BenchmarkMetric = "intelligence" | "coding" | "agentic";

export interface BenchmarkRanking {
  id: string;
  name: string;
  score: number;
}

export interface PerformanceRanking {
  id: string;
  name: string;
  throughput: number;
  latencyMs: number;
  provider: string;
  pricePerMillion: number | null;
}

export interface AppRanking {
  id: string;
  name: string;
  description: string;
  websiteUrl: string;
  tokens: number;
  requests: number;
  rank: number;
  categories: string[];
}

export interface SessionCostRanking {
  id: string;
  harness: string;
  medianUsd: number;
}

type JsonRecord = Record<string, unknown>;

function record(value: unknown): JsonRecord | null {
  return value != null && typeof value === "object" && !Array.isArray(value)
    ? (value as JsonRecord)
    : null;
}

function array(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}

function number(value: unknown): number | null {
  if (typeof value === "number" && Number.isFinite(value)) return value;
  if (typeof value === "string" && value.trim()) {
    const parsed = Number(value);
    return Number.isFinite(parsed) ? parsed : null;
  }
  return null;
}

function string(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value : null;
}

function externalHttpUrl(...values: unknown[]): string | null {
  for (const value of values) {
    const candidate = string(value);
    if (!candidate) continue;
    try {
      const url = new URL(candidate);
      if (url.protocol === "http:" || url.protocol === "https:") {
        return url.toString();
      }
    } catch {
      // Ignore malformed upstream URLs and try the next official source.
    }
  }
  return null;
}

function unwrapPayload(envelope: OpenRouterRankingsEnvelope): JsonRecord {
  return record(envelope.payload) ?? {};
}

function sanitizeSeries(value: unknown): Record<string, number> {
  const source = record(value);
  if (!source) return {};
  return Object.fromEntries(
    Object.entries(source).flatMap(([key, item]) => {
      const parsed = number(item);
      return parsed == null ? [] : [[key, parsed]];
    }),
  );
}

function pointsFromSeriesRows(value: unknown): RankingPoint[] {
  return array(value)
    .flatMap((item) => {
      const row = record(item);
      const x = string(row?.x);
      if (!row || !x) return [];
      return [{ x, ys: sanitizeSeries(row.ys) }];
    })
    .filter((point) => Object.keys(point.ys).length > 0)
    .sort((a, b) => a.x.localeCompare(b.x));
}

function rowMetric(row: JsonRecord, unit: UsageRankingView["unit"]): number {
  if (unit === "subrequests") return number(row.count) ?? 0;
  const total = number(row.total_tokens);
  if (total != null) return total;
  return (
    (number(row.total_prompt_tokens) ?? 0) +
    (number(row.total_completion_tokens) ?? 0)
  );
}

function pointsFromModelRows(
  value: unknown,
  unit: UsageRankingView["unit"],
): { points: RankingPoint[]; changes: Map<string, number> } {
  const grouped = new Map<string, Record<string, number>>();
  const changeRows = new Map<string, { date: string; value: number }>();
  for (const item of array(value)) {
    const row = record(item);
    const date = string(row?.date);
    const id = string(row?.variant_permaslug) ?? string(row?.model_permaslug);
    if (!row || !date || !id || id === "other") continue;
    const values = grouped.get(date) ?? {};
    values[id] = (values[id] ?? 0) + rowMetric(row, unit);
    grouped.set(date, values);
    const change = number(row.change);
    const previousChange = changeRows.get(id);
    if (change != null && (!previousChange || date > previousChange.date)) {
      changeRows.set(id, { date, value: change });
    }
  }
  return {
    points: [...grouped.entries()]
      .map(([x, ys]) => ({ x, ys }))
      .sort((a, b) => a.x.localeCompare(b.x)),
    changes: new Map(
      [...changeRows.entries()].map(([id, item]) => [id, item.value]),
    ),
  };
}

function rankingFromPoints(
  points: RankingPoint[],
  changes = new Map<string, number>(),
): RankingItem[] {
  const totals = new Map<string, number>();
  for (const point of points) {
    for (const [id, value] of Object.entries(point.ys)) {
      if (id.toLowerCase() === "others") continue;
      totals.set(id, (totals.get(id) ?? 0) + value);
    }
  }
  return [...totals.entries()]
    .filter(([, value]) => value > 0)
    .map(([id, value]) => ({ id, value, change: changes.get(id) ?? null }))
    .sort((a, b) => b.value - a.value);
}

function rankingFromLatestPoint(points: RankingPoint[]): RankingItem[] {
  const latest = points[points.length - 1];
  const previous = points[points.length - 2];
  if (!latest) return [];
  return (Object.entries(latest.ys) as Array<[string, number]>)
    .filter(([id, value]) => id.toLowerCase() !== "others" && value > 0)
    .map(([id, value]) => {
      const prior = previous?.ys[id];
      const change =
        prior != null && prior > 0 ? ((value - prior) / prior) * 100 : null;
      return { id, value, change };
    })
    .sort((a, b) => b.value - a.value);
}

export function normalizeUsageRankings(
  envelope: OpenRouterRankingsEnvelope,
): UsageRankingView {
  const payload = unwrapPayload(envelope);
  const unit =
    envelope.dataset === "batch"
      ? "subrequests"
      : envelope.dataset === "text"
        ? "tokens"
        : "requests";

  if (envelope.dataset === "modality") {
    const container = record(payload.data);
    const points = pointsFromSeriesRows(container?.data);
    return { points, ranking: rankingFromLatestPoint(points), unit };
  }

  const rawRows = payload.data;
  const series = pointsFromSeriesRows(rawRows);
  if (series.length > 0) {
    return { points: series, ranking: rankingFromPoints(series), unit };
  }

  const { points, changes } = pointsFromModelRows(rawRows, unit);
  return { points, ranking: rankingFromPoints(points, changes), unit };
}

export function normalizeToolRankings(
  envelope: OpenRouterRankingsEnvelope,
): UsageRankingView {
  const points = pointsFromSeriesRows(unwrapPayload(envelope).data);
  return {
    points,
    ranking: rankingFromLatestPoint(points),
    unit: "requests",
  };
}

export function normalizeTasks(
  envelope: OpenRouterRankingsEnvelope,
  metric: "spend" | "tokens" = "spend",
): TaskRankingsView {
  const payload = unwrapPayload(envelope);
  const data = record(payload.data) ?? {};

  if (envelope.dataSource === "official") {
    const categories = array(data.macro_categories).flatMap((item) => {
      const row = record(item);
      const id = string(row?.key);
      if (!row || !id) return [];
      return [
        {
          id,
          label: string(row.label) ?? id,
          share: number(row.token_share) ?? number(row.usage_share) ?? 0,
        },
      ];
    });
    const tasks = array(data.classifications)
      .flatMap((item) => {
        const row = record(item);
        const id = string(row?.tag);
        if (!row || !id) return [];
        const models = array(row.models).flatMap((modelItem) => {
          const model = record(modelItem);
          const modelId = string(model?.id);
          if (!model || !modelId) return [];
          return [
            {
              id: modelId,
              share:
                number(model.tag_token_share) ??
                number(model.tag_usage_share) ??
                0,
              change: null,
            },
          ];
        });
        return [
          {
            id,
            label: string(row.display_name) ?? id,
            category: string(row.macro_category) ?? "general",
            share: number(row.token_share) ?? number(row.usage_share) ?? 0,
            models,
          },
        ];
      })
      .sort((a, b) => b.share - a.share);
    return {
      windowDays: number(data.window_days) ?? 7,
      categories,
      tasks,
      metric: "tokens",
    };
  }

  const selected =
    record(data[metric]) ?? record(data.spend) ?? record(data.tokens) ?? {};
  const categories = array(selected.macroCategories).flatMap((item) => {
    const row = record(item);
    const id = string(row?.key);
    if (!row || !id) return [];
    return [
      {
        id,
        label: string(row.label) ?? id,
        share: number(row.spendShare) ?? 0,
      },
    ];
  });
  const tasks = array(selected.tasks)
    .flatMap((item) => {
      const row = record(item);
      const id = string(row?.tag);
      if (!row || !id) return [];
      return [
        {
          id,
          label: id.replace(/_/g, " ").replace(":", " · "),
          category: string(row.macroCategory) ?? "general",
          share: number(row.spendShareOfTotal) ?? 0,
          models: array(row.models).flatMap((modelItem) => {
            const model = record(modelItem);
            const modelId = string(model?.model);
            if (!model || !modelId) return [];
            return [
              {
                id: modelId,
                share: number(model.share) ?? 0,
                change: number(model.deltaPp),
              },
            ];
          }),
        },
      ];
    })
    .sort((a, b) => b.share - a.share);
  return {
    windowDays: number(selected.windowDays) ?? 30,
    categories,
    tasks,
    metric: record(data[metric])
      ? metric
      : record(data.spend)
        ? "spend"
        : "tokens",
  };
}

export function normalizeBenchmarks(
  envelope: OpenRouterRankingsEnvelope,
  metric: BenchmarkMetric,
): BenchmarkRanking[] {
  const payload = unwrapPayload(envelope);
  const data = payload.data;
  if (envelope.dataSource === "official") {
    const scoreKey = `${metric}_index`;
    return array(data)
      .flatMap((item) => {
        const row = record(item);
        const id = string(row?.model_permaslug);
        const score = number(row?.[scoreKey]);
        if (!row || !id || score == null) return [];
        return [{ id, name: string(row.display_name) ?? id, score }];
      })
      .sort((a, b) => b.score - a.score);
  }

  const aaData = record(record(data)?.aaData);
  return array(aaData?.[metric])
    .flatMap((item) => {
      const row = record(item);
      const id =
        string(row?.heuristic_openrouter_slug) ??
        string(row?.openrouter_slug) ??
        string(row?.permaslug);
      const score = number(row?.score);
      if (!row || !id || score == null) return [];
      return [{ id, name: string(row.aa_name) ?? id, score }];
    })
    .sort((a, b) => b.score - a.score);
}

export function normalizePerformance(
  envelope: OpenRouterRankingsEnvelope,
): PerformanceRanking[] {
  return array(unwrapPayload(envelope).data)
    .flatMap((item) => {
      const row = record(item);
      const id = string(row?.slug) ?? string(row?.id);
      const throughput = number(row?.p50_throughput);
      const latencyMs = number(row?.p50_latency);
      if (!row || !id || throughput == null || latencyMs == null) return [];
      return [
        {
          id,
          name: string(row.name) ?? id,
          throughput,
          latencyMs,
          provider:
            string(row.best_throughput_provider) ?? string(row.author) ?? "",
          pricePerMillion: number(row.best_throughput_price),
        },
      ];
    })
    .sort((a, b) => b.throughput - a.throughput);
}

export function normalizeApps(
  envelope: OpenRouterRankingsEnvelope,
): AppRanking[] {
  const payload = unwrapPayload(envelope);
  const data = payload.data;
  const rows =
    envelope.dataSource === "official"
      ? array(data)
      : array(record(data)?.week);
  return rows
    .flatMap((item) => {
      const row = record(item);
      const app = record(row?.app);
      const id =
        string(app?.slug) ??
        (number(row?.app_id) ?? number(app?.id))?.toString() ??
        string(row?.app_name);
      const websiteUrl = externalHttpUrl(
        app?.origin_url,
        app?.main_url,
        app?.source_code_url,
        row?.origin_url,
        row?.main_url,
        row?.source_code_url,
      );
      if (!row || !id || !websiteUrl) return [];
      return [
        {
          id,
          name: string(row.app_name) ?? string(app?.title) ?? id,
          description: string(app?.description) ?? "",
          websiteUrl,
          tokens: number(row.total_tokens) ?? 0,
          requests: number(row.total_requests) ?? 0,
          rank: number(row.rank) ?? 0,
          categories: array(app?.categories).filter(
            (value): value is string => typeof value === "string",
          ),
        },
      ];
    })
    .sort((a, b) => a.rank - b.rank || b.tokens - a.tokens);
}

export function normalizeSessionCosts(
  envelope: OpenRouterRankingsEnvelope,
): SessionCostRanking[] {
  const payload = unwrapPayload(envelope);
  const data = record(payload.data) ?? {};
  return array(data.harnesses)
    .flatMap((harnessItem) => {
      const harness = record(harnessItem);
      const label = string(harness?.label) ?? "Agent";
      return array(harness?.models).flatMap((modelItem) => {
        const model = record(modelItem);
        const id = string(model?.model);
        const corePoint = array(model?.points)
          .map(record)
          .find((point) => string(point?.bucket) === "core");
        const medianUsd = number(corePoint?.medianUsd);
        if (!model || !id || medianUsd == null) return [];
        return [{ id, harness: label, medianUsd }];
      });
    })
    .sort((a, b) => a.medianUsd - b.medianUsd);
}

export function humanNumber(value: number): string {
  const absolute = Math.abs(value);
  if (absolute >= 1_000_000_000_000)
    return `${(value / 1_000_000_000_000).toFixed(2)}T`;
  if (absolute >= 1_000_000_000)
    return `${(value / 1_000_000_000).toFixed(2)}B`;
  if (absolute >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  if (absolute >= 1_000) return `${(value / 1_000).toFixed(1)}K`;
  return value.toLocaleString();
}
