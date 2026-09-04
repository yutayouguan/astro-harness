import { invoke } from "@tauri-apps/api/core";
import {
  Activity,
  AppWindow,
  AudioLines,
  BarChart3,
  Binary,
  Gauge,
  Image,
  Layers3,
  ListChecks,
  RefreshCw,
  Sparkles,
  Trophy,
  Video,
  WalletCards,
  Wrench,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useI18n } from "../../i18n/LocaleContext";
import {
  humanNumber,
  isRankingsEnvelopeFresh,
  normalizeApps,
  normalizeBenchmarks,
  normalizePerformance,
  normalizeSessionCosts,
  normalizeTasks,
  normalizeToolRankings,
  normalizeUsageRankings,
  type BenchmarkMetric,
  type OpenRouterRankingsEnvelope,
  type RankingItem,
  type RankingPoint,
} from "../../lib/model/modelRankings";
import { ModelBrandIcon } from "../icons/ProviderIcons";

type RankingsSection =
  | "usage"
  | "tools"
  | "tasks"
  | "performance"
  | "benchmarks"
  | "apps"
  | "session_cost";

type UsageModality =
  | "text"
  | "image"
  | "embeddings"
  | "rerank"
  | "video"
  | "speech"
  | "transcription"
  | "batch";

type RequestTarget = {
  dataset: string;
  modality: string | null;
};

const SECTION_ITEMS: Array<{
  id: RankingsSection;
  Icon: typeof BarChart3;
}> = [
  { id: "usage", Icon: BarChart3 },
  { id: "tools", Icon: Wrench },
  { id: "tasks", Icon: ListChecks },
  { id: "performance", Icon: Gauge },
  { id: "benchmarks", Icon: Trophy },
  { id: "apps", Icon: AppWindow },
  { id: "session_cost", Icon: WalletCards },
];

const MODALITY_ITEMS: Array<{
  id: UsageModality;
  Icon: typeof Sparkles;
}> = [
  { id: "text", Icon: Sparkles },
  { id: "image", Icon: Image },
  { id: "embeddings", Icon: Binary },
  { id: "rerank", Icon: ListChecks },
  { id: "video", Icon: Video },
  { id: "speech", Icon: AudioLines },
  { id: "transcription", Icon: Activity },
  { id: "batch", Icon: Layers3 },
];

const SERIES_COLORS = ["#8b5cf6", "#0ea5e9", "#10b981", "#f59e0b", "#f43f5e"];
const rankingsMemoryCache = new Map<string, OpenRouterRankingsEnvelope>();

function requestTarget(
  section: RankingsSection,
  modality: UsageModality,
): RequestTarget {
  if (section !== "usage") return { dataset: section, modality: null };
  if (modality === "text" || modality === "batch") {
    return { dataset: modality, modality: null };
  }
  return { dataset: "modality", modality };
}

function requestKey(target: RequestTarget): string {
  return `${target.dataset}:${target.modality ?? ""}`;
}

function shortModelName(id: string): string {
  const slash = id.indexOf("/");
  return slash >= 0 ? id.slice(slash + 1) : id;
}

function formatDate(value: string | null): string {
  if (!value) return "—";
  const normalized = value.includes("T")
    ? value
    : value.includes(" ")
      ? `${value.replace(" ", "T")}Z`
      : `${value}T00:00:00Z`;
  const date = new Date(normalized);
  if (Number.isNaN(date.getTime())) return value.slice(0, 10);
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: value.length > 10 ? "2-digit" : undefined,
    minute: value.length > 10 ? "2-digit" : undefined,
  }).format(date);
}

function TrendChart({
  points,
  ranking,
  unit,
}: {
  points: RankingPoint[];
  ranking: RankingItem[];
  unit: string;
}) {
  const width = 720;
  const height = 250;
  const inset = { top: 16, right: 18, bottom: 28, left: 48 };
  const series = ranking.slice(0, 5);
  const values = points.flatMap((point) =>
    series.map((item) => point.ys[item.id] ?? 0),
  );
  const max = Math.max(1, ...values);
  const x = (index: number) =>
    inset.left +
    (index / Math.max(1, points.length - 1)) *
      (width - inset.left - inset.right);
  const y = (value: number) =>
    height - inset.bottom - (value / max) * (height - inset.top - inset.bottom);

  if (points.length === 0) return null;

  return (
    <div className="mm-rank-chart-wrap">
      <svg
        className="mm-rank-chart"
        viewBox={`0 0 ${width} ${height}`}
        role="img"
        aria-label={`OpenRouter ${unit} trend`}
      >
        {[0, 0.25, 0.5, 0.75, 1].map((ratio) => {
          const lineY = y(max * ratio);
          return (
            <g key={ratio}>
              <line
                x1={inset.left}
                x2={width - inset.right}
                y1={lineY}
                y2={lineY}
                className="mm-rank-grid-line"
              />
              <text x={inset.left - 8} y={lineY + 4} textAnchor="end">
                {humanNumber(max * ratio)}
              </text>
            </g>
          );
        })}
        {series.map((item, seriesIndex) => {
          const path = points
            .map((point, pointIndex) => {
              const command = pointIndex === 0 ? "M" : "L";
              return `${command}${x(pointIndex)},${y(point.ys[item.id] ?? 0)}`;
            })
            .join(" ");
          return (
            <path
              key={item.id}
              d={path}
              fill="none"
              stroke={SERIES_COLORS[seriesIndex]}
              strokeWidth="2.5"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          );
        })}
        <text x={inset.left} y={height - 7} textAnchor="start">
          {points[0]?.x.slice(0, 10)}
        </text>
        <text x={width - inset.right} y={height - 7} textAnchor="end">
          {points[points.length - 1]?.x.slice(0, 10)}
        </text>
      </svg>
      <div className="mm-rank-legend" aria-label="Trend series">
        {series.map((item, index) => (
          <span key={item.id}>
            <i style={{ background: SERIES_COLORS[index] }} />
            {item.label ?? shortModelName(item.id)}
          </span>
        ))}
      </div>
    </div>
  );
}

function RankedList({
  items,
  valueLabel,
  emptyLabel,
}: {
  items: RankingItem[];
  valueLabel: (value: number) => string;
  emptyLabel: string;
}) {
  const top = items.slice(0, 10);
  const max = top[0]?.value || 1;
  if (top.length === 0) {
    return <div className="mm-rank-empty">{emptyLabel}</div>;
  }
  return (
    <div className="mm-rank-list" role="list">
      {top.map((item, index) => (
        <div className="mm-rank-row" role="listitem" key={item.id}>
          <span className="mm-rank-position">{index + 1}</span>
          <ModelBrandIcon modelId={item.id} width={22} height={22} />
          <div className="mm-rank-identity">
            <span title={item.id}>{item.label ?? shortModelName(item.id)}</span>
            <div className="mm-rank-bar" aria-hidden="true">
              <i
                style={{ width: `${Math.max(2, (item.value / max) * 100)}%` }}
              />
            </div>
          </div>
          <strong>{valueLabel(item.value)}</strong>
          {item.change != null && (
            <span className={item.change >= 0 ? "is-up" : "is-down"}>
              {item.change >= 0 ? "+" : ""}
              {item.change.toFixed(1)}%
            </span>
          )}
        </div>
      ))}
    </div>
  );
}

function LoadingState({ label }: { label: string }) {
  return (
    <div className="mm-rank-loading" aria-live="polite">
      <div className="mm-rank-loading-bars" aria-hidden="true">
        {Array.from({ length: 9 }, (_, index) => (
          <i key={index} style={{ animationDelay: `${index * -70}ms` }} />
        ))}
      </div>
      <span>{label}</span>
    </div>
  );
}

export default function ModelRankingsPanel({ active }: { active: boolean }) {
  const { t } = useI18n();
  const [section, setSection] = useState<RankingsSection>("usage");
  const [modality, setModality] = useState<UsageModality>("text");
  const [benchmarkMetric, setBenchmarkMetric] =
    useState<BenchmarkMetric>("intelligence");
  const [taskMetric, setTaskMetric] = useState<"spend" | "tokens">("spend");
  const [responses, setResponses] = useState<
    Record<string, OpenRouterRankingsEnvelope>
  >(() => Object.fromEntries(rankingsMemoryCache.entries()));
  const [loadingKeys, setLoadingKeys] = useState<Record<string, boolean>>({});
  const [errors, setErrors] = useState<Record<string, string>>({});
  const attemptedAt = useRef(new Map<string, number>());

  const target = useMemo(
    () => requestTarget(section, modality),
    [section, modality],
  );
  const key = requestKey(target);
  const envelope = responses[key] ?? null;

  const load = useCallback(
    async (forceRefresh: boolean) => {
      attemptedAt.current.set(key, Date.now());
      setLoadingKeys((current) => ({ ...current, [key]: true }));
      setErrors((current) => {
        const next = { ...current };
        delete next[key];
        return next;
      });
      try {
        const result = await invoke<OpenRouterRankingsEnvelope>(
          "get_openrouter_rankings",
          {
            dataset: target.dataset,
            modality: target.modality,
            forceRefresh,
          },
        );
        rankingsMemoryCache.set(key, result);
        setResponses((current) => ({ ...current, [key]: result }));
      } catch (reason) {
        setErrors((current) => ({ ...current, [key]: String(reason) }));
      } finally {
        setLoadingKeys((current) => ({ ...current, [key]: false }));
      }
    },
    [key, target.dataset, target.modality],
  );

  useEffect(() => {
    const lastAttempt = attemptedAt.current.get(key) ?? 0;
    if (
      !active ||
      (envelope && isRankingsEnvelopeFresh(envelope)) ||
      Date.now() - lastAttempt < 60_000
    )
      return;
    void load(false);
  }, [active, envelope, key, load]);

  const usage = useMemo(() => {
    if (!envelope || section !== "usage") return null;
    return normalizeUsageRankings(envelope);
  }, [envelope, section]);
  const tools = useMemo(() => {
    if (!envelope || section !== "tools") return null;
    return normalizeToolRankings(envelope);
  }, [envelope, section]);
  const tasks = useMemo(() => {
    if (!envelope || section !== "tasks") return null;
    return normalizeTasks(envelope, taskMetric);
  }, [envelope, section, taskMetric]);
  const [selectedTask, setSelectedTask] = useState<string | null>(null);
  const activeTask =
    tasks?.tasks.find((task) => task.id === selectedTask) ??
    tasks?.tasks[0] ??
    null;
  const performance = useMemo(() => {
    if (!envelope || section !== "performance") return [];
    return normalizePerformance(envelope);
  }, [envelope, section]);
  const benchmarks = useMemo(() => {
    if (!envelope || section !== "benchmarks") return [];
    return normalizeBenchmarks(envelope, benchmarkMetric);
  }, [benchmarkMetric, envelope, section]);
  const apps = useMemo(() => {
    if (!envelope || section !== "apps") return [];
    return normalizeApps(envelope);
  }, [envelope, section]);
  const sessionCosts = useMemo(() => {
    if (!envelope || section !== "session_cost") return [];
    return normalizeSessionCosts(envelope);
  }, [envelope, section]);

  if (!active) return null;

  const busy = loadingKeys[key] === true;
  const error = errors[key] ?? null;
  const sourceLabel = envelope
    ? envelope.dataSource === "official"
      ? t("modelRankings.source.official" as never)
      : t("modelRankings.source.frontend" as never)
    : null;

  return (
    <div className="mm-rankings-workspace">
      <div className="mm-rankings-commandbar">
        <div className="mm-rankings-sections" role="tablist">
          {SECTION_ITEMS.map(({ id, Icon }) => (
            <button
              key={id}
              type="button"
              role="tab"
              aria-selected={section === id}
              className={section === id ? "active" : ""}
              onClick={() => setSection(id)}
            >
              <Icon size={14} />
              {t(`modelRankings.section.${id}` as never)}
            </button>
          ))}
        </div>
        <button
          type="button"
          className="model-market-refresh"
          onClick={() => void load(true)}
          disabled={busy}
          title={t("modelMarket.refresh")}
        >
          <RefreshCw size={14} className={busy ? "spin" : ""} />
        </button>
      </div>

      {section === "usage" && (
        <div className="mm-rank-modality-tabs" role="tablist">
          {MODALITY_ITEMS.map(({ id, Icon }) => (
            <button
              key={id}
              type="button"
              role="tab"
              aria-selected={modality === id}
              className={modality === id ? "active" : ""}
              onClick={() => setModality(id)}
            >
              <Icon size={14} />
              {t(`modelRankings.modality.${id}` as never)}
            </button>
          ))}
        </div>
      )}

      <div className="mm-rank-statusbar" aria-live="polite">
        <span>{t(`modelRankings.title.${section}` as never)}</span>
        <span className="mm-rank-status-spacer" />
        {envelope && (
          <>
            <span className={`mm-rank-source source-${envelope.dataSource}`}>
              {sourceLabel}
            </span>
            <span>
              {envelope.freshness === "stale"
                ? t("modelRankings.status.stale" as never)
                : t("modelRankings.status.updated" as never)}{" "}
              {formatDate(envelope.asOf ?? envelope.fetchedAt)}
            </span>
          </>
        )}
      </div>

      {busy && !envelope && (
        <LoadingState label={t("modelRankings.loading" as never)} />
      )}
      {error && !envelope && (
        <div className="mm-rank-error">
          <strong>{t("modelRankings.error.title" as never)}</strong>
          <span>{error}</span>
          <button type="button" onClick={() => void load(true)}>
            {t("modelRankings.retry" as never)}
          </button>
        </div>
      )}

      {section === "usage" && usage && (
        <div className="mm-rank-primary-grid">
          <section className="mm-rank-surface mm-rank-trend-surface">
            <header>
              <div>
                <h3>{t("modelRankings.usage.trend" as never)}</h3>
                <p>{t(`modelRankings.unit.${usage.unit}` as never)}</p>
              </div>
            </header>
            <TrendChart
              points={usage.points}
              ranking={usage.ranking}
              unit={usage.unit}
            />
          </section>
          <section className="mm-rank-surface">
            <header>
              <div>
                <h3>{t("modelRankings.usage.top" as never)}</h3>
                <p>{t("modelRankings.usage.window" as never)}</p>
              </div>
            </header>
            <RankedList
              items={usage.ranking}
              valueLabel={(value) => `${humanNumber(value)} ${usage.unit}`}
              emptyLabel={t("modelRankings.empty" as never)}
            />
          </section>
        </div>
      )}

      {section === "tools" && tools && (
        <div className="mm-rank-primary-grid">
          <section className="mm-rank-surface mm-rank-trend-surface">
            <header>
              <div>
                <h3>{t("modelRankings.tools.trend" as never)}</h3>
                <p>{t("modelRankings.tools.subtitle" as never)}</p>
              </div>
            </header>
            <TrendChart
              points={tools.points}
              ranking={tools.ranking}
              unit="tool calls"
            />
          </section>
          <section className="mm-rank-surface">
            <header>
              <h3>{t("modelRankings.tools.top" as never)}</h3>
            </header>
            <RankedList
              items={tools.ranking}
              valueLabel={(value) => humanNumber(value)}
              emptyLabel={t("modelRankings.empty" as never)}
            />
          </section>
        </div>
      )}

      {section === "tasks" && tasks && (
        <div className="mm-task-layout">
          {envelope?.dataSource === "frontend" && (
            <div className="mm-task-metric-row">
              <span>{t("modelRankings.tasks.metric" as never)}</span>
              <div className="mm-rank-inline-tabs">
                {(["spend", "tokens"] as const).map((metric) => (
                  <button
                    key={metric}
                    type="button"
                    className={taskMetric === metric ? "active" : ""}
                    onClick={() => setTaskMetric(metric)}
                  >
                    {t(`modelRankings.tasks.${metric}` as never)}
                  </button>
                ))}
              </div>
            </div>
          )}
          <div className="mm-task-categories">
            {tasks.categories.map((category) => (
              <div key={category.id}>
                <span>{category.label}</span>
                <strong>{(category.share * 100).toFixed(1)}%</strong>
              </div>
            ))}
          </div>
          <div className="mm-task-split">
            <div className="mm-task-list" role="list">
              {tasks.tasks.map((task) => (
                <button
                  key={task.id}
                  type="button"
                  className={activeTask?.id === task.id ? "active" : ""}
                  onClick={() => setSelectedTask(task.id)}
                >
                  <span>{task.label}</span>
                  <strong>{(task.share * 100).toFixed(1)}%</strong>
                </button>
              ))}
            </div>
            <section className="mm-rank-surface">
              <header>
                <div>
                  <h3>{activeTask?.label ?? "—"}</h3>
                  <p>
                    {tasks.windowDays}d · {tasks.metric}
                  </p>
                </div>
              </header>
              <RankedList
                items={(activeTask?.models ?? []).map((model) => ({
                  id: model.id,
                  value: model.share,
                  change: model.change,
                }))}
                valueLabel={(value) => `${(value * 100).toFixed(1)}%`}
                emptyLabel={t("modelRankings.empty" as never)}
              />
            </section>
          </div>
        </div>
      )}

      {section === "performance" && envelope && (
        <section className="mm-rank-surface mm-rank-table-surface">
          <header>
            <div>
              <h3>{t("modelRankings.performance.title" as never)}</h3>
              <p>{t("modelRankings.performance.subtitle" as never)}</p>
            </div>
          </header>
          <div className="mm-rank-table" role="table">
            <div className="mm-rank-table-row mm-rank-table-head" role="row">
              <span>#</span>
              <span />
              <span>{t("modelRankings.column.model" as never)}</span>
              <span>{t("modelRankings.column.throughput" as never)}</span>
              <span>{t("modelRankings.column.latency" as never)}</span>
              <span>{t("modelRankings.column.provider" as never)}</span>
              <span>{t("modelRankings.column.price" as never)}</span>
            </div>
            {performance.slice(0, 30).map((item, index) => (
              <div className="mm-rank-table-row" role="row" key={item.id}>
                <span>{index + 1}</span>
                <ModelBrandIcon modelId={item.id} width={22} height={22} />
                <strong title={item.id}>{shortModelName(item.id)}</strong>
                <span>{humanNumber(item.throughput)} tok/s</span>
                <span>{humanNumber(item.latencyMs)} ms</span>
                <span>{item.provider || "—"}</span>
                <span>
                  {item.pricePerMillion == null
                    ? "—"
                    : `$${item.pricePerMillion.toFixed(2)}/M`}
                </span>
              </div>
            ))}
          </div>
        </section>
      )}

      {section === "benchmarks" && envelope && (
        <section className="mm-rank-surface mm-rank-table-surface">
          <header>
            <div>
              <h3>{t("modelRankings.benchmarks.title" as never)}</h3>
              <p>{t("modelRankings.benchmarks.subtitle" as never)}</p>
            </div>
            <div className="mm-rank-inline-tabs">
              {(["intelligence", "coding", "agentic"] as const).map(
                (metric) => (
                  <button
                    key={metric}
                    type="button"
                    className={benchmarkMetric === metric ? "active" : ""}
                    onClick={() => setBenchmarkMetric(metric)}
                  >
                    {t(`modelRankings.benchmark.${metric}` as never)}
                  </button>
                ),
              )}
            </div>
          </header>
          <RankedList
            items={benchmarks.slice(0, 20).map((item) => ({
              id: item.id,
              label: item.name,
              value: item.score,
              change: null,
            }))}
            valueLabel={(value) => value.toFixed(1)}
            emptyLabel={t("modelRankings.empty" as never)}
          />
        </section>
      )}

      {section === "apps" && envelope && (
        <section className="mm-rank-surface mm-app-rankings">
          <header>
            <div>
              <h3>{t("modelRankings.apps.title" as never)}</h3>
              <p>{t("modelRankings.apps.subtitle" as never)}</p>
            </div>
          </header>
          <div className="mm-app-grid">
            {apps.slice(0, 20).map((app, index) => (
              <article key={app.id}>
                <span className="mm-rank-position">
                  {app.rank || index + 1}
                </span>
                <span className="mm-app-icon" aria-hidden="true">
                  <AppWindow size={19} />
                  {app.iconUrl && (
                    <img
                      src={app.iconUrl}
                      alt=""
                      loading="lazy"
                      referrerPolicy="no-referrer"
                      onError={(event) => {
                        event.currentTarget.hidden = true;
                      }}
                    />
                  )}
                </span>
                <div>
                  <h4>{app.name}</h4>
                  {app.categories.length > 0 && (
                    <p>{app.categories.join(" · ")}</p>
                  )}
                  {app.description && <span>{app.description}</span>}
                </div>
                <strong>{humanNumber(app.tokens)}</strong>
              </article>
            ))}
          </div>
        </section>
      )}

      {section === "session_cost" && envelope && (
        <section className="mm-rank-surface mm-rank-table-surface">
          <header>
            <div>
              <h3>{t("modelRankings.cost.title" as never)}</h3>
              <p>{t("modelRankings.cost.subtitle" as never)}</p>
            </div>
          </header>
          <div className="mm-rank-table mm-cost-table" role="table">
            <div className="mm-rank-table-row mm-rank-table-head" role="row">
              <span>#</span>
              <span />
              <span>{t("modelRankings.column.model" as never)}</span>
              <span>{t("modelRankings.column.app" as never)}</span>
              <span>{t("modelRankings.column.median" as never)}</span>
            </div>
            {sessionCosts.slice(0, 30).map((item, index) => (
              <div
                className="mm-rank-table-row"
                role="row"
                key={`${item.harness}:${item.id}`}
              >
                <span>{index + 1}</span>
                <ModelBrandIcon modelId={item.id} width={22} height={22} />
                <strong title={item.id}>{shortModelName(item.id)}</strong>
                <span>{item.harness}</span>
                <span>${item.medianUsd.toFixed(3)}</span>
              </div>
            ))}
          </div>
        </section>
      )}

      {envelope && (
        <footer className="mm-rank-attribution">
          Source: OpenRouter (openrouter.ai/rankings), as of{" "}
          {formatDate(envelope.asOf ?? envelope.fetchedAt)}. Licensed under CC
          BY 4.0.
        </footer>
      )}
    </div>
  );
}
