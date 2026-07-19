/** 用量洞察面板：模型用量 / 工具技能 / Agent 协作 / Tracing 分 Tab。 */
import { useEffect, useMemo, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Activity,
  AlertTriangle,
  BarChart3,
  Bot,
  Building2,
  Calendar,
  CalendarDays,
  CalendarRange,
  ChevronRight,
  Coins,
  Cpu,
  DollarSign,
  GitBranch,
  Layers,
  LayoutDashboard,
  Network,
  Puzzle,
  Timer,
  UserRound,
  Wrench,
} from "lucide-react";
import { useActiveAgent } from "../../hooks/app/useActiveAgent";
import { useI18n } from "../../i18n/LocaleContext";
import type { Locale, MessageKey } from "../../i18n/messages";
import {
  aggregateByProvider,
  DEFAULT_INSIGHTS_VIEW,
  INSIGHTS_VIEW_ORDER,
  needsUsageInsights,
  providerSpendTop,
  type InsightsViewMode,
} from "../../lib/insights/insightsView";
import {
  groupEventsForTraceDisplay,
  shortTurnId,
  traceSessionTitle,
  turnGroupTitle,
} from "../../lib/chat/traceTurnGroups";
import McpIcon from "../icons/McpIcon";
import { ModelBrandIcon, ProviderBrandIcon } from "../icons/ProviderIcons";

type Period = "month" | "quarter" | "year";
type Metric = "calls" | "tokens" | "cost";
type ViewMode = InsightsViewMode;

type RankItem = {
  kind: string;
  name: string;
  calls: number;
  tokens: number;
  cost_usd: number;
};

type UsageInsights = {
  kpis: { calls: number; tokens: number; cost_usd: number; active_agents: number };
  series: { bucket: string; calls: number; tokens: number; cost_usd: number }[];
  rankings: {
    by_kind: RankItem[];
    by_agent: RankItem[];
    by_model: RankItem[];
  };
  unpriced_llm_events?: number;
};

type CollaborationStep = {
  seq: number;
  role: string;
  agent_id: string | null;
  status: string;
  output: string | null;
  error: string | null;
};

type CollaborationOrchestration = {
  id: string;
  goal: string;
  status: string;
  parent_agent_id: string;
  session_id: string | null;
  created_at: string;
  updated_at: string;
  finished_at: string | null;
  error: string | null;
  result_summary: string | null;
  steps: CollaborationStep[];
};

type CollaborationNode = {
  id: string;
  label: string;
  kind: string;
};

type CollaborationEdge = {
  from: string;
  to: string;
  weight: number;
};

type CollaborationInsights = {
  orchestrations: CollaborationOrchestration[];
  graph: {
    nodes: CollaborationNode[];
    edges: CollaborationEdge[];
  };
};

type TraceEvent = {
  id: string;
  ts: string;
  kind: string;
  name: string;
  agent_id: string;
  input_tokens: number;
  output_tokens: number;
  total_tokens: number;
  cost_usd: number;
  duration_ms?: number | null;
  turn_id?: string | null;
  parent_id?: string | null;
  status?: string | null;
  input?: string | null;
  output?: string | null;
};

type TraceSummary = {
  session_id: string;
  agent_id: string;
  title?: string;
  started_at: string;
  ended_at: string;
  event_count: number;
  tokens: number;
  cost_usd: number;
  kinds: string[];
  events: TraceEvent[];
};

type TraceInsights = {
  kpis: {
    traces: number;
    events: number;
    llm: number;
    tools: number;
    skills: number;
  };
  traces: TraceSummary[];
};

const PERIOD_TABS: {
  id: Period;
  labelKey: MessageKey;
  Icon: typeof Calendar;
}[] = [
  { id: "month", labelKey: "insights.period.month", Icon: Calendar },
  { id: "quarter", labelKey: "insights.period.quarter", Icon: CalendarRange },
  { id: "year", labelKey: "insights.period.year", Icon: CalendarDays },
];

const METRIC_TABS: { id: Metric; labelKey: MessageKey }[] = [
  { id: "calls", labelKey: "insights.metric.calls" },
  { id: "tokens", labelKey: "insights.metric.tokens" },
  { id: "cost", labelKey: "insights.metric.cost" },
];

const VIEW_TAB_META: Record<
  ViewMode,
  { labelKey: MessageKey; Icon: typeof BarChart3 }
> = {
  overview: { labelKey: "insights.view.overview", Icon: LayoutDashboard },
  models: { labelKey: "insights.view.models", Icon: Cpu },
  tools: { labelKey: "insights.view.tools", Icon: Wrench },
  collab: { labelKey: "insights.view.collab", Icon: Network },
  tracing: { labelKey: "insights.view.tracing", Icon: Activity },
};

const VIEW_TABS = INSIGHTS_VIEW_ORDER.map((id) => ({
  id,
  ...VIEW_TAB_META[id],
}));

function seriesValue(
  s: UsageInsights["series"][number],
  metric: Metric,
): number {
  if (metric === "tokens") return s.tokens;
  if (metric === "cost") return s.cost_usd;
  return s.calls;
}

function formatSeriesTip(
  s: UsageInsights["series"][number],
  metric: Metric,
): string {
  if (metric === "tokens") return `${s.bucket}: ${formatTokens(s.tokens)}`;
  if (metric === "cost") return `${s.bucket}: ${formatCost(s.cost_usd)}`;
  return `${s.bucket}: ${s.calls}`;
}

function formatMetricTotal(total: number, metric: Metric): string {
  if (metric === "tokens") return formatTokens(total);
  if (metric === "cost") return formatCost(total);
  return String(total);
}

/** 柱高用像素，避免百分比在 flex 里塌成贴底细线；非零值保底可见。 */
const CHART_PLOT_H = 128;
function barHeightPx(value: number, maxVal: number): number {
  if (value <= 0 || maxVal <= 0) return 0;
  const raw = (value / maxVal) * CHART_PLOT_H;
  return Math.max(10, Math.round(raw));
}

/** 多数桶接近 0 或峰值相对分布极偏时，提示已放大柱高。 */
function isSparseSeries(values: number[], maxVal: number): boolean {
  if (maxVal <= 0) return false;
  const nonzero = values.filter((v) => v > 0);
  if (nonzero.length === 0) return false;
  const mean = nonzero.reduce((a, b) => a + b, 0) / nonzero.length;
  return nonzero.length <= 2 || mean / maxVal < 0.22;
}

function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function formatCost(usd: number): string {
  if (usd <= 0) return "$0";
  if (usd < 0.01) return `$${usd.toFixed(4)}`;
  return `$${usd.toFixed(2)}`;
}

function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

const EN_MONTHS = [
  "Jan",
  "Feb",
  "Mar",
  "Apr",
  "May",
  "Jun",
  "Jul",
  "Aug",
  "Sep",
  "Oct",
  "Nov",
  "Dec",
] as const;

/** 按 period / locale 格式化桶标签（后端：月=YYYY-MM-DD，季/年=YYYY-MM） */
function formatBucketLabel(bucket: string, period: Period, locale: Locale): string {
  if (period === "month") {
    const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(bucket);
    if (m) {
      const month = Number(m[2]);
      const day = Number(m[3]);
      return locale === "zh" ? `${day}日` : `${month}/${day}`;
    }
  } else {
    const m = /^(\d{4})-(\d{2})$/.exec(bucket);
    if (m) {
      const month = Number(m[2]);
      if (locale === "zh") return `${month}月`;
      return EN_MONTHS[month - 1] ?? bucket;
    }
  }
  return bucket;
}

function statusClass(status: string): string {
  const s = status.toLowerCase();
  if (s === "done" || s === "completed" || s === "success") return "status-done";
  if (s === "failed" || s === "error") return "status-failed";
  if (s === "running" || s === "in_progress") return "status-running";
  if (s === "pending" || s === "queued") return "status-pending";
  return "status-other";
}

function KindIcon({ kind }: { kind: string }) {
  const props = { size: 13, strokeWidth: 2.25, "aria-hidden": true as const };
  switch (kind) {
    case "tool":
      return <Wrench {...props} />;
    case "skill":
      return <Puzzle {...props} />;
    case "mcp":
      return <McpIcon size={13} />;
    case "cron":
      return <Timer {...props} />;
    case "llm":
      return <Cpu {...props} />;
    case "user":
      return <UserRound {...props} />;
    case "provider":
      return <Building2 {...props} />;
    case "agent":
      return <Bot {...props} />;
    default:
      return <Layers {...props} />;
  }
}

function RankIdentityIcon({
  type,
  name,
}: {
  type: "model" | "provider" | "kind";
  name: string;
}) {
  if (type === "model") {
    return <ModelBrandIcon modelId={name} size={14} />;
  }
  if (type === "provider") {
    return <ProviderBrandIcon kind={name} size={14} />;
  }
  return <KindIcon kind={name} />;
}

function CollabGraphSvg({
  nodes,
  edges,
}: {
  nodes: CollaborationNode[];
  edges: CollaborationEdge[];
}) {
  const w = 520;
  const h = 360;
  const cx = w / 2;
  const cy = h / 2;
  const R = Math.min(w, h) * 0.32;
  const pos = new Map(
    nodes.map((n, i) => {
      const a = (2 * Math.PI * i) / Math.max(nodes.length, 1) - Math.PI / 2;
      return [n.id, { x: cx + R * Math.cos(a), y: cy + R * Math.sin(a) }] as const;
    }),
  );
  const maxW = Math.max(1, ...edges.map((e) => e.weight));
  return (
    <svg viewBox={`0 0 ${w} ${h}`} className="insights-collab-graph">
      {edges.map((e) => {
        const a = pos.get(e.from);
        const b = pos.get(e.to);
        if (!a || !b) return null;
        const sw = 1 + (3 * e.weight) / maxW;
        return (
          <g key={`${e.from}->${e.to}`}>
            <line
              x1={a.x}
              y1={a.y}
              x2={b.x}
              y2={b.y}
              strokeWidth={sw}
              className="insights-collab-edge"
            />
            <title>{`${e.from} → ${e.to}: ${e.weight}`}</title>
          </g>
        );
      })}
      {nodes.map((n) => {
        const p = pos.get(n.id)!;
        const label = n.label.length > 10 ? `${n.label.slice(0, 9)}…` : n.label;
        return (
          <g key={n.id}>
            <circle
              cx={p.x}
              cy={p.y}
              r={22}
              className={`insights-collab-node kind-${n.kind}`}
            />
            <title>{n.label}</title>
            <text
              x={p.x}
              y={p.y + 4}
              textAnchor="middle"
              className="insights-collab-label"
            >
              {label}
            </text>
          </g>
        );
      })}
    </svg>
  );
}

function InsightsTrendChart({
  title,
  series,
  metric,
  onMetricChange,
  maxVal,
  period,
  locale,
  emptyMessage,
  t,
}: {
  title: string;
  series: UsageInsights["series"];
  metric: Metric;
  onMetricChange: (m: Metric) => void;
  maxVal: number;
  period: Period;
  locale: Locale;
  emptyMessage: string;
  t: (key: MessageKey, vars?: Record<string, string>) => string;
}) {
  const values = series.map((s) => seriesValue(s, metric));
  const total = values.reduce((a, b) => a + b, 0);
  const hasSignal = total > 0;
  const sparse = hasSignal && isSparseSeries(values, maxVal);

  return (
    <div className="insights-chart-wrap insights-models-chart">
      <div className="insights-chart-heading">
        <div className="insights-chart-heading-label">
          <BarChart3 size={15} strokeWidth={2.25} aria-hidden />
          <span>{title}</span>
          {hasSignal && (
            <span className="insights-chart-total">
              {t("insights.chart.periodTotal", {
                v: formatMetricTotal(total, metric),
              })}
            </span>
          )}
        </div>
        <div className="insights-seg insights-seg--sm insights-metric-tabs" role="tablist">
          {METRIC_TABS.map(({ id, labelKey }) => (
            <button
              key={id}
              type="button"
              role="tab"
              className={`insights-seg-item insights-metric-tab${metric === id ? " active" : ""}`}
              aria-selected={metric === id}
              onClick={() => onMetricChange(id)}
            >
              {t(labelKey)}
            </button>
          ))}
        </div>
      </div>
      {series.length > 0 && hasSignal ? (
        <div className="insights-chart" aria-label={`${metric} trend`}>
          {sparse && (
            <p className="insights-chart-sparse-hint">{t("insights.chart.sparse")}</p>
          )}
          {series.map((s) => {
            const v = seriesValue(s, metric);
            return (
              <div
                key={s.bucket}
                className="insights-bar-col"
                title={formatSeriesTip(s, metric)}
              >
                <div className="insights-bar-plot" style={{ height: CHART_PLOT_H }}>
                  <div
                    className={`insights-bar${v > 0 ? "" : " is-empty"}`}
                    style={{ height: barHeightPx(v, maxVal) }}
                  />
                </div>
                <span className="insights-bar-label">
                  {formatBucketLabel(s.bucket, period, locale)}
                </span>
              </div>
            );
          })}
        </div>
      ) : (
        <div className="insights-panel-empty insights-chart-empty">
          <p>{emptyMessage}</p>
        </div>
      )}
    </div>
  );
}

export default function InsightsPanel({ active }: { active: boolean }) {
  const { t, locale } = useI18n();
  const { activeAgentId: agentId } = useActiveAgent();
  const [view, setView] = useState<ViewMode>(DEFAULT_INSIGHTS_VIEW);
  const [period, setPeriod] = useState<Period>("month");
  const [metric, setMetric] = useState<Metric>("tokens");
  const [data, setData] = useState<UsageInsights | null>(null);
  const [collab, setCollab] = useState<CollaborationInsights | null>(null);
  const [traces, setTraces] = useState<TraceInsights | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [selectedTraceId, setSelectedTraceId] = useState<string | null>(null);
  const [expandedTurns, setExpandedTurns] = useState<Record<string, boolean>>({});
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!active || !isTauri() || !needsUsageInsights(view)) return;
    let cancelled = false;
    void (async () => {
      try {
        const res = await invoke<UsageInsights>("get_usage_insights", {
          args: {
            period,
            as_of: null,
            agent_id: agentId,
          },
        });
        if (!cancelled) {
          setData(res);
          setError(null);
        }
      } catch (e) {
        if (!cancelled) setError(String(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [active, period, agentId, view]);

  useEffect(() => {
    if (!active || !isTauri() || view !== "collab") return;
    let cancelled = false;
    void (async () => {
      try {
        const res = await invoke<CollaborationInsights>("get_collaboration_insights", {
          args: {
            period,
            as_of: null,
            agent_id: agentId,
          },
        });
        if (!cancelled) {
          setCollab(res);
          setSelectedId((prev) => {
            if (prev && res.orchestrations.some((o) => o.id === prev)) return prev;
            return res.orchestrations[0]?.id ?? null;
          });
          setError(null);
        }
      } catch (e) {
        if (!cancelled) setError(String(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [active, period, agentId, view]);

  useEffect(() => {
    if (!active || !isTauri() || view !== "tracing") return;
    let cancelled = false;
    void (async () => {
      try {
        const res = await invoke<TraceInsights>("get_trace_insights", {
          args: {
            period,
            as_of: null,
            agent_id: agentId,
          },
        });
        if (!cancelled) {
          setTraces(res);
          setSelectedTraceId((prev) => {
            if (prev && res.traces.some((t) => t.session_id === prev)) return prev;
            return res.traces[0]?.session_id ?? null;
          });
          setError(null);
        }
      } catch (e) {
        if (!cancelled) setError(String(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [active, period, agentId, view]);

  const maxVal = Math.max(
    1,
    ...(data?.series.map((s) => seriesValue(s, metric)) ?? [1]),
  );
  const byProvider = useMemo(
    () => aggregateByProvider(data?.rankings.by_model ?? []),
    [data],
  );
  const modelStats = useMemo(() => {
    const models = data?.rankings.by_model ?? [];
    return {
      modelCount: models.length,
      agentCount: data?.rankings.by_agent.length ?? 0,
    };
  }, [data]);
  const overviewProviderTop = useMemo(
    () => providerSpendTop(byProvider, 5),
    [byProvider],
  );
  const overviewProviderMax = Math.max(
    1,
    ...overviewProviderTop.map((r) => (r.cost_usd > 0 ? r.cost_usd : r.tokens)),
  );
  const overviewUseCost = overviewProviderTop.some((r) => r.cost_usd > 0);
  const modelBars = useMemo(() => {
    const items = [...(data?.rankings.by_model ?? [])]
      .sort((a, b) => b.tokens - a.tokens || b.calls - a.calls)
      .slice(0, 8);
    const max = Math.max(1, ...items.map((r) => r.tokens || r.calls));
    return { items, max };
  }, [data]);
  const toolRanks = useMemo(
    () => (data?.rankings.by_kind ?? []).filter((r) => r.kind === "tool"),
    [data],
  );
  const skillRanks = useMemo(
    () => (data?.rankings.by_kind ?? []).filter((r) => r.kind === "skill"),
    [data],
  );
  const mcpRanks = useMemo(
    () => (data?.rankings.by_kind ?? []).filter((r) => r.kind === "mcp"),
    [data],
  );
  const cronRanks = useMemo(
    () => (data?.rankings.by_kind ?? []).filter((r) => r.kind === "cron"),
    [data],
  );
  const toolCallTotal = useMemo(
    () => toolRanks.reduce((s, r) => s + r.calls, 0),
    [toolRanks],
  );
  const skillCallTotal = useMemo(
    () => skillRanks.reduce((s, r) => s + r.calls, 0),
    [skillRanks],
  );
  const mcpCallTotal = useMemo(
    () => mcpRanks.reduce((s, r) => s + r.calls, 0),
    [mcpRanks],
  );
  const cronCallTotal = useMemo(
    () => cronRanks.reduce((s, r) => s + r.calls, 0),
    [cronRanks],
  );
  const toolSeriesMax = Math.max(
    1,
    ...(data?.series.map((s) => s.calls) ?? [1]),
  );
  const topInvocationBars = useMemo(() => {
    const all = [...toolRanks, ...skillRanks, ...mcpRanks, ...cronRanks]
      .slice()
      .sort((a, b) => b.calls - a.calls)
      .slice(0, 8);
    const max = Math.max(1, ...all.map((r) => r.calls));
    return { items: all, max };
  }, [toolRanks, skillRanks, mcpRanks, cronRanks]);

  const modelsEmpty =
    data && data.rankings.by_model.length === 0 && data.kpis.tokens === 0;
  const hasUnpriced = (data?.unpriced_llm_events ?? 0) > 0;

  const collabStats = useMemo(() => {
    const orch = collab?.orchestrations ?? [];
    let done = 0;
    let failed = 0;
    let active = 0;
    for (const o of orch) {
      const s = o.status.toLowerCase();
      if (s === "done" || s === "completed" || s === "success") done += 1;
      else if (s === "failed" || s === "error") failed += 1;
      else active += 1;
    }
    return {
      total: orch.length,
      done,
      failed,
      active,
      nodes: collab?.graph.nodes.length ?? 0,
      edges: collab?.graph.edges.length ?? 0,
    };
  }, [collab]);

  const selected =
    collab?.orchestrations.find((o) => o.id === selectedId) ?? null;
  const selectedTrace =
    traces?.traces.find((t) => t.session_id === selectedTraceId) ?? null;
  const turnGroups = useMemo(
    () =>
      selectedTrace ? groupEventsForTraceDisplay(selectedTrace.events) : [],
    [selectedTrace],
  );

  useEffect(() => {
    setExpandedTurns({});
  }, [selectedTraceId]);

  return (
    <div className="insights-panel">
      <div className="insights-toolbar">
        <div
          className="insights-seg insights-view-tabs"
          role="tablist"
          aria-label="insights view"
        >
          {VIEW_TABS.map(({ id, labelKey, Icon }) => (
            <button
              key={id}
              type="button"
              role="tab"
              className={`insights-seg-item insights-view-tab${view === id ? " active" : ""}`}
              aria-selected={view === id}
              onClick={() => setView(id)}
            >
              <Icon size={15} strokeWidth={2.25} aria-hidden />
              {t(labelKey)}
            </button>
          ))}
        </div>
        <div className="insights-seg insights-period-tabs" role="tablist">
          {PERIOD_TABS.map(({ id, labelKey, Icon }) => (
            <button
              key={id}
              type="button"
              role="tab"
              className={`insights-seg-item insights-period-tab${period === id ? " active" : ""}`}
              aria-selected={period === id}
              onClick={() => setPeriod(id)}
            >
              <Icon size={15} strokeWidth={2.25} aria-hidden />
              {t(labelKey)}
            </button>
          ))}
        </div>
      </div>

      {error && <p className="insights-error">{error}</p>}

      {view === "overview" && data && (
        <>
          {hasUnpriced && (
            <div className="insights-unpriced" role="status">
              <AlertTriangle size={15} strokeWidth={2.25} aria-hidden />
              <div className="insights-unpriced-copy">
                <strong>{t("insights.unpriced")}</strong>
                <span>{t("insights.unpriced.hint")}</span>
              </div>
            </div>
          )}

          <div className="insights-kpis insights-kpis-overview">
            <KpiCard
              emphasis={data.kpis.cost_usd <= 0 ? "muted" : "default"}
              icon={<DollarSign size={16} strokeWidth={2.25} aria-hidden />}
              label={t("insights.kpi.cost")}
              value={formatCost(data.kpis.cost_usd)}
            />
            <KpiCard
              emphasis="primary"
              icon={<Coins size={16} strokeWidth={2.25} aria-hidden />}
              label={t("insights.kpi.tokens")}
              value={formatTokens(data.kpis.tokens)}
            />
            <KpiCard
              icon={<Activity size={16} strokeWidth={2.25} aria-hidden />}
              label={t("insights.kpi.calls")}
              value={String(data.kpis.calls)}
            />
          </div>

          <div className="insights-overview-grid">
            <InsightsTrendChart
              title={t("insights.chart.usageTrend")}
              series={data.series}
              metric={metric}
              onMetricChange={setMetric}
              maxVal={maxVal}
              period={period}
              locale={locale}
              emptyMessage={t("insights.chart.empty")}
              t={t}
            />

            <section className="insights-hbar-panel">
              <div className="insights-rank-title-row">
                <h3 className="insights-rank-title">
                  <Building2 size={14} strokeWidth={2.25} aria-hidden />
                  {t("insights.rank.providerSpend")}
                </h3>
                <button
                  type="button"
                  className="insights-more-btn"
                  onClick={() => setView("models")}
                >
                  {t("insights.rank.more")}
                </button>
              </div>
              {overviewProviderTop.length === 0 ? (
                <p className="insights-rank-empty">{t("insights.rank.empty")}</p>
              ) : (
                <ul className="insights-hbar-list">
                  {overviewProviderTop.map((r, idx) => {
                    const val = overviewUseCost ? r.cost_usd : r.tokens;
                    return (
                      <li
                        key={r.name}
                        className={`insights-hbar-item${idx < 3 ? ` rank-${idx + 1}` : ""}`}
                      >
                        <span className="insights-hbar-label">
                          <span className="insights-hbar-rank" aria-hidden>
                            {idx + 1}
                          </span>
                          <span className="insights-rank-kind-icon" title="provider">
                            <ProviderBrandIcon kind={r.name} size={14} />
                          </span>
                          {r.name}
                        </span>
                        <div className="insights-hbar-track">
                          <div
                            className="insights-hbar-fill"
                            style={{
                              width: `${(val / overviewProviderMax) * 100}%`,
                            }}
                          />
                        </div>
                        <span className="insights-hbar-value">
                          {overviewUseCost
                            ? formatCost(r.cost_usd)
                            : formatTokens(r.tokens)}
                        </span>
                      </li>
                    );
                  })}
                </ul>
              )}
            </section>
          </div>

          {data.series.length === 0 && overviewProviderTop.length === 0 && (
            <p className="insights-panel-hint">{t("insights.empty.overview")}</p>
          )}
        </>
      )}

      {view === "models" && data && (
        <>
          <div className="insights-kpis insights-kpis-models-secondary">
            <KpiCard
              icon={<Layers size={16} strokeWidth={2.25} aria-hidden />}
              label={t("insights.kpi.models")}
              value={String(modelStats.modelCount)}
            />
            <KpiCard
              icon={<Bot size={16} strokeWidth={2.25} aria-hidden />}
              label={t("insights.kpi.agents")}
              value={String(modelStats.agentCount || data.kpis.active_agents)}
            />
          </div>

          <section className="insights-hbar-panel">
            <h3 className="insights-rank-title">
              <Cpu size={14} strokeWidth={2.25} aria-hidden />
              {t("insights.rank.modelTop")}
            </h3>
            {modelBars.items.length === 0 ? (
              <p className="insights-rank-empty">{t("insights.rank.empty")}</p>
            ) : (
              <ul className="insights-hbar-list">
                {modelBars.items.map((r) => (
                  <li key={r.name} className="insights-hbar-item">
                    <span className="insights-hbar-label">
                      <span className="insights-rank-kind-icon" title="llm">
                        <ModelBrandIcon modelId={r.name} size={14} />
                      </span>
                      {r.name}
                    </span>
                    <div className="insights-hbar-track">
                      <div
                        className="insights-hbar-fill"
                        style={{
                          width: `${((r.tokens || r.calls) / modelBars.max) * 100}%`,
                        }}
                      />
                    </div>
                    <span className="insights-hbar-value">
                      {r.tokens > 0
                        ? `${formatTokens(r.tokens)} · ${formatCost(r.cost_usd)}`
                        : r.calls}
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </section>

          <div className="insights-ranks">
            <RankList
              title={t("insights.rank.provider")}
              icon={<Building2 size={14} strokeWidth={2.25} aria-hidden />}
              items={byProvider}
              identityType="provider"
              showCost
              showTokens
              emptyHint={t("insights.rank.empty")}
            />
            <RankList
              title={t("insights.rank.model")}
              icon={<Cpu size={14} strokeWidth={2.25} aria-hidden />}
              items={data.rankings.by_model}
              identityType="model"
              showCost
              showTokens
              emptyHint={t("insights.rank.empty")}
            />
            <RankList
              title={t("insights.rank.agent")}
              icon={<Bot size={14} strokeWidth={2.25} aria-hidden />}
              items={data.rankings.by_agent}
              showCost
              showTokens
              emptyHint={t("insights.rank.empty")}
            />
          </div>

          {modelsEmpty && (
            <p className="insights-panel-hint">{t("insights.empty.modelsHint")}</p>
          )}
        </>
      )}

      {view === "tools" && data && (
        <>
          <div className="insights-kpis">
            <KpiCard
              icon={<Wrench size={16} strokeWidth={2.25} aria-hidden />}
              label={t("insights.kpi.tools")}
              value={String(toolCallTotal)}
            />
            <KpiCard
              icon={<Puzzle size={16} strokeWidth={2.25} aria-hidden />}
              label={t("insights.kpi.skills")}
              value={String(skillCallTotal)}
            />
            <KpiCard
              icon={<McpIcon size={16} />}
              label={t("insights.kpi.mcp")}
              value={String(mcpCallTotal)}
            />
            <KpiCard
              icon={<Timer size={16} strokeWidth={2.25} aria-hidden />}
              label={t("insights.kpi.cron")}
              value={String(cronCallTotal)}
            />
          </div>

          {data.series.length > 0 && (
            <div className="insights-chart-wrap">
              <div className="insights-chart-heading">
                <div className="insights-chart-heading-label">
                  <BarChart3 size={15} strokeWidth={2.25} aria-hidden />
                  <span>{t("insights.chart.toolCalls")}</span>
                </div>
              </div>
              <div className="insights-chart" aria-label="calls trend">
                {data.series.map((s) => (
                  <div
                    key={s.bucket}
                    className="insights-bar-col"
                    title={`${s.bucket}: ${s.calls}`}
                  >
                    <div className="insights-bar-plot" style={{ height: CHART_PLOT_H }}>
                      <div
                        className={`insights-bar${s.calls > 0 ? "" : " is-empty"}`}
                        style={{
                          height: barHeightPx(s.calls, toolSeriesMax),
                        }}
                      />
                    </div>
                    <span className="insights-bar-label">
                      {formatBucketLabel(s.bucket, period, locale)}
                    </span>
                  </div>
                ))}
              </div>
            </div>
          )}

          {topInvocationBars.items.length > 0 && (
            <section className="insights-hbar-panel">
              <h3 className="insights-rank-title">
                <Layers size={14} strokeWidth={2.25} aria-hidden />
                {t("insights.rank.kind")}
              </h3>
              <ul className="insights-hbar-list">
                {topInvocationBars.items.map((r) => (
                  <li key={`${r.kind}:${r.name}`} className="insights-hbar-item">
                    <span className="insights-hbar-label">
                      <span className="insights-rank-kind-icon" title={r.kind}>
                        <KindIcon kind={r.kind} />
                      </span>
                      {r.name}
                    </span>
                    <div className="insights-hbar-track">
                      <div
                        className="insights-hbar-fill"
                        style={{
                          width: `${(r.calls / topInvocationBars.max) * 100}%`,
                        }}
                      />
                    </div>
                    <span className="insights-hbar-value">{r.calls}</span>
                  </li>
                ))}
              </ul>
            </section>
          )}

          <div className="insights-ranks insights-ranks-tools">
            <RankList
              title={t("insights.rank.tool")}
              icon={<Wrench size={14} strokeWidth={2.25} aria-hidden />}
              items={toolRanks}
              emptyHint={t("insights.rank.empty")}
            />
            <RankList
              title={t("insights.rank.skill")}
              icon={<Puzzle size={14} strokeWidth={2.25} aria-hidden />}
              items={skillRanks}
              emptyHint={t("insights.rank.empty")}
            />
            <RankList
              title={t("insights.rank.mcp")}
              icon={<McpIcon size={14} />}
              items={mcpRanks}
              emptyHint={t("insights.rank.empty")}
            />
            <RankList
              title={t("insights.rank.cron")}
              icon={<Timer size={14} strokeWidth={2.25} aria-hidden />}
              items={cronRanks}
              emptyHint={t("insights.rank.empty")}
            />
          </div>

          {toolCallTotal + skillCallTotal + mcpCallTotal + cronCallTotal === 0 && (
            <p className="insights-panel-hint">{t("insights.empty.toolsHint")}</p>
          )}
        </>
      )}

      {view === "collab" && collab && (
        <>
          <div className="insights-kpis insights-kpis-collab">
            <KpiCard
              icon={<GitBranch size={16} strokeWidth={2.25} aria-hidden />}
              label={t("insights.kpi.orch")}
              value={String(collabStats.total)}
            />
            <KpiCard
              icon={<Layers size={16} strokeWidth={2.25} aria-hidden />}
              label={t("insights.kpi.orchDone")}
              value={String(collabStats.done)}
            />
            <KpiCard
              icon={<AlertTriangle size={16} strokeWidth={2.25} aria-hidden />}
              label={t("insights.kpi.orchFailed")}
              value={String(collabStats.failed)}
            />
            <KpiCard
              icon={<Network size={16} strokeWidth={2.25} aria-hidden />}
              label={t("insights.kpi.orchActive")}
              value={String(collabStats.active)}
            />
            <KpiCard
              icon={<Bot size={16} strokeWidth={2.25} aria-hidden />}
              label={t("insights.kpi.graphNodes")}
              value={String(collabStats.nodes)}
            />
            <KpiCard
              icon={<GitBranch size={16} strokeWidth={2.25} aria-hidden />}
              label={t("insights.kpi.graphEdges")}
              value={String(collabStats.edges)}
            />
          </div>

          <div className="insights-collab-layout">
            <div className="insights-collab-left">
              <section className="insights-collab-list-panel">
                <h3 className="insights-collab-section-title">
                  <GitBranch size={14} strokeWidth={2.25} aria-hidden />
                  {t("insights.collab.listTitle")}
                </h3>
                {collab.orchestrations.length === 0 ? (
                  <div className="insights-panel-empty">
                    <p>{t("insights.collab.listEmpty")}</p>
                    <p className="insights-panel-hint">{t("insights.collab.emptyHint")}</p>
                  </div>
                ) : (
                  <ul className="insights-collab-list">
                    {collab.orchestrations.map((o) => (
                      <li key={o.id}>
                        <button
                          type="button"
                          className={`insights-collab-list-item${selectedId === o.id ? " active" : ""} ${statusClass(o.status)}`}
                          onClick={() => setSelectedId(o.id)}
                        >
                          <span className="insights-collab-list-goal">{o.goal || o.id}</span>
                          <span className="insights-collab-list-meta">
                            <span className={`insights-collab-status ${statusClass(o.status)}`}>
                              {o.status}
                            </span>
                            <span className="insights-collab-list-agent">{o.parent_agent_id}</span>
                          </span>
                        </button>
                      </li>
                    ))}
                  </ul>
                )}
              </section>

              <section className="insights-collab-steps-panel">
                <h3 className="insights-collab-section-title">
                  <Layers size={14} strokeWidth={2.25} aria-hidden />
                  {t("insights.collab.steps")}
                </h3>
                {!selected ? (
                  <p className="insights-collab-hint">{t("insights.collab.noSelection")}</p>
                ) : selected.steps.length === 0 ? (
                  <p className="insights-collab-hint">{t("insights.collab.stepsEmpty")}</p>
                ) : (
                  <div className="insights-collab-steps-strip">
                    {selected.steps.map((step, i) => (
                      <div key={`${step.seq}-${step.role}`} className="insights-collab-step-wrap">
                        {i > 0 && <span className="insights-collab-step-arrow" aria-hidden>→</span>}
                        <details className={`insights-collab-step ${statusClass(step.status)}`}>
                          <summary>
                            <span className="insights-collab-step-role">{step.role}</span>
                            <span className={`insights-collab-status ${statusClass(step.status)}`}>
                              {step.status}
                            </span>
                          </summary>
                          {(step.output || step.error || step.agent_id) && (
                            <div className="insights-collab-step-detail">
                              {step.agent_id && <p>agent: {step.agent_id}</p>}
                              {step.output && <pre>{step.output}</pre>}
                              {step.error && <pre className="insights-collab-step-error">{step.error}</pre>}
                            </div>
                          )}
                        </details>
                      </div>
                    ))}
                  </div>
                )}
              </section>
            </div>

            <section className="insights-collab-graph-panel">
              <h3 className="insights-collab-section-title">
                <Network size={14} strokeWidth={2.25} aria-hidden />
                {t("insights.collab.graphTitle")}
              </h3>
              {collab.graph.nodes.length === 0 && collab.graph.edges.length === 0 ? (
                <div className="insights-panel-empty insights-collab-graph-empty">
                  <span className="insights-empty-icon" aria-hidden>
                    <Network size={28} strokeWidth={1.75} />
                  </span>
                  <p>{t("insights.collab.graphEmpty")}</p>
                  <p className="insights-panel-hint">{t("insights.collab.emptyHint")}</p>
                </div>
              ) : (
                <CollabGraphSvg
                  nodes={collab.graph.nodes}
                  edges={collab.graph.edges}
                />
              )}
            </section>
          </div>
        </>
      )}

      {view === "tracing" && traces && (
        <>
          <div className="insights-kpis">
            <KpiCard
              icon={<Activity size={16} strokeWidth={2.25} aria-hidden />}
              label={t("insights.trace.kpi.traces")}
              value={String(traces.kpis.traces)}
            />
            <KpiCard
              icon={<Layers size={16} strokeWidth={2.25} aria-hidden />}
              label={t("insights.trace.kpi.events")}
              value={String(traces.kpis.events)}
            />
            <KpiCard
              icon={<Cpu size={16} strokeWidth={2.25} aria-hidden />}
              label={t("insights.trace.kpi.llm")}
              value={String(traces.kpis.llm)}
            />
            <KpiCard
              icon={<Wrench size={16} strokeWidth={2.25} aria-hidden />}
              label={t("insights.trace.kpi.tools")}
              value={String(traces.kpis.tools)}
            />
          </div>

          <div className="insights-trace-layout">
            <section className="insights-trace-list-panel">
              <h3 className="insights-collab-section-title">
                <Activity size={14} strokeWidth={2.25} aria-hidden />
                {t("insights.trace.listTitle")}
              </h3>
              {traces.traces.length === 0 ? (
                <div className="insights-panel-empty">
                  <p>{t("insights.trace.listEmpty")}</p>
                  <p className="insights-panel-hint">{t("insights.trace.emptyHint")}</p>
                </div>
              ) : (
                <ul className="insights-collab-list">
                  {traces.traces.map((tr) => (
                    <li key={tr.session_id}>
                      <button
                        type="button"
                        className={`insights-collab-list-item${selectedTraceId === tr.session_id ? " active" : ""}`}
                        onClick={() => setSelectedTraceId(tr.session_id)}
                      >
                        <span
                          className="insights-collab-list-goal"
                          title={tr.session_id}
                        >
                          {traceSessionTitle(
                            tr.title,
                            t("insights.trace.unnamedSession"),
                          )}
                        </span>
                        <span className="insights-collab-list-meta">
                          <span className="insights-collab-list-agent">{tr.agent_id}</span>
                          <span className="insights-trace-meta-chip">
                            {formatTraceTime(tr.started_at)} · {tr.event_count} ·{" "}
                            {formatTokens(tr.tokens)}
                          </span>
                        </span>
                        <span className="insights-trace-kinds">
                          {tr.kinds.map((k) => (
                            <span key={k} className={`insights-trace-kind kind-${k}`}>
                              {k}
                            </span>
                          ))}
                        </span>
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </section>

            <section className="insights-trace-chain-panel">
              <h3 className="insights-collab-section-title">
                <GitBranch size={14} strokeWidth={2.25} aria-hidden />
                {t("insights.trace.chainTitle")}
              </h3>
              {!selectedTrace ? (
                <p className="insights-collab-hint">{t("insights.trace.noSelection")}</p>
              ) : selectedTrace.events.length === 0 ? (
                <p className="insights-collab-hint">{t("insights.trace.listEmpty")}</p>
              ) : (
                <ol className="insights-turn-groups">
                  {turnGroups.map((g) => {
                    const open = !!expandedTurns[g.turnKey];
                    const label = turnGroupTitle(
                      g.events,
                      g.turn_id,
                      t("insights.trace.unlabeledTurn"),
                      t("insights.trace.turnGroup"),
                    );
                    const turnHint = g.turn_id
                      ? shortTurnId(g.turn_id, t("insights.trace.unlabeledTurn"))
                      : undefined;
                    return (
                      <li key={g.turnKey} className="insights-turn-group">
                        <button
                          type="button"
                          className="insights-turn-group-head"
                          aria-expanded={open}
                          title={turnHint ? `${label} · ${turnHint}` : label}
                          onClick={() =>
                            setExpandedTurns((s) => ({
                              ...s,
                              [g.turnKey]: !s[g.turnKey],
                            }))
                          }
                        >
                          <span className="insights-turn-group-label">{label}</span>
                          <span className="insights-turn-group-meta">
                            {g.events.length} · {formatTokens(g.tokens)} tok ·{" "}
                            {formatCost(g.cost_usd)}
                          </span>
                        </button>
                        {open && (
                          <ol className="insights-trace-timeline insights-trace-timeline-nested">
                            {g.events.map((ev, i) => (
                              <TraceEventItem
                                key={ev.id}
                                event={ev}
                                isLast={i === g.events.length - 1}
                                t={t}
                              />
                            ))}
                          </ol>
                        )}
                      </li>
                    );
                  })}
                </ol>
              )}
            </section>
          </div>
        </>
      )}
    </div>
  );
}

function formatTraceDuration(ms: number): string {
  if (ms < 1_000) return `${Math.max(0, Math.round(ms))} ms`;
  if (ms < 60_000) return `${(ms / 1_000).toFixed(ms < 10_000 ? 1 : 0)} s`;
  const minutes = Math.floor(ms / 60_000);
  const seconds = Math.round((ms % 60_000) / 1_000);
  return `${minutes}m ${seconds}s`;
}

function traceStatusLabel(
  status: string | null | undefined,
  t: (key: MessageKey) => string,
): string | null {
  if (!status) return null;
  if (status === "ok" || status === "done" || status === "success") {
    return t("insights.trace.status.done");
  }
  if (status === "error" || status === "failed") {
    return t("insights.trace.status.error");
  }
  if (status === "running") return t("insights.trace.status.running");
  return status;
}

function TraceEventItem({
  event,
  isLast,
  t,
}: {
  event: TraceEvent;
  isLast: boolean;
  t: (key: MessageKey, vars?: Record<string, string>) => string;
}) {
  const name =
    event.kind === "user" ? t("insights.trace.event.user") : event.name;
  const status = traceStatusLabel(event.status, t);
  const duration =
    event.duration_ms != null
      ? formatTraceDuration(event.duration_ms)
      : t("insights.trace.durationUnavailable");

  return (
    <li className={`insights-trace-event kind-${event.kind}`}>
      <span className="insights-trace-rail" aria-hidden>
        <span className="insights-trace-dot">
          <KindIcon kind={event.kind} />
        </span>
        {!isLast && <span className="insights-trace-line" />}
      </span>
      <details className="insights-trace-event-details">
        <summary className="insights-trace-event-summary">
          <span className="insights-trace-event-chevron" aria-hidden>
            <ChevronRight size={14} strokeWidth={2.25} />
          </span>
          <span className="insights-trace-event-summary-main">
            <span className="insights-trace-event-head">
              <span className="insights-trace-event-name">{name}</span>
              <span className={`insights-trace-kind kind-${event.kind}`}>
                {event.kind}
              </span>
            </span>
            <span className="insights-trace-event-meta">
              <span>{formatTraceTime(event.ts)}</span>
              <span>{duration}</span>
              {event.total_tokens > 0 && (
                <span>{formatTokens(event.total_tokens)} tok</span>
              )}
              {status && <span>{status}</span>}
            </span>
          </span>
        </summary>

        <div className="insights-trace-event-detail">
          <div className="insights-trace-event-facts">
            <span>
              <strong>{t("insights.trace.detail.duration")}</strong>
              {duration}
            </span>
            <span>
              <strong>{t("insights.trace.detail.inputTokens")}</strong>
              {formatTokens(event.input_tokens)}
            </span>
            <span>
              <strong>{t("insights.trace.detail.outputTokens")}</strong>
              {formatTokens(event.output_tokens)}
            </span>
            <span>
              <strong>{t("insights.trace.detail.totalTokens")}</strong>
              {formatTokens(event.total_tokens)}
            </span>
            <span>
              <strong>{t("insights.trace.detail.cost")}</strong>
              {formatCost(event.cost_usd)}
            </span>
            {event.agent_id && (
              <span>
                <strong>Agent</strong>
                {event.agent_id}
              </span>
            )}
          </div>

          {event.input ? (
            <section className="insights-trace-io-block">
              <h4>{t("insights.trace.detail.input")}</h4>
              <pre>{event.input}</pre>
            </section>
          ) : (
            <section className="insights-trace-io-block is-empty">
              <h4>{t("insights.trace.detail.input")}</h4>
              <p>{t("insights.trace.detail.noData")}</p>
            </section>
          )}

          {event.output ? (
            <section className="insights-trace-io-block">
              <h4>{t("insights.trace.detail.output")}</h4>
              <pre>{event.output}</pre>
            </section>
          ) : (
            <section className="insights-trace-io-block is-empty">
              <h4>{t("insights.trace.detail.output")}</h4>
              <p>{t("insights.trace.detail.noData")}</p>
            </section>
          )}
        </div>
      </details>
    </li>
  );
}

function formatTraceTime(ts: string): string {
  const m = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})/.exec(ts);
  if (!m) return ts;
  return `${m[2]}-${m[3]} ${m[4]}:${m[5]}:${m[6]}`;
}

function KpiCard({
  icon,
  label,
  value,
  emphasis = "default",
}: {
  icon: ReactNode;
  label: string;
  value: string;
  emphasis?: "default" | "primary" | "muted";
}) {
  return (
    <div className={`insights-kpi insights-kpi--${emphasis}`}>
      <span className="insights-kpi-label">
        <span className="insights-kpi-icon">{icon}</span>
        {label}
      </span>
      <span className="insights-kpi-value">{value}</span>
    </div>
  );
}

function RankList({
  title,
  icon,
  items,
  showCost,
  showTokens,
  emptyHint,
  identityType = "kind",
}: {
  title: string;
  icon: ReactNode;
  items: RankItem[];
  showCost?: boolean;
  showTokens?: boolean;
  emptyHint?: string;
  identityType?: "model" | "provider" | "kind";
}) {
  return (
    <section className="insights-rank">
      <h3 className="insights-rank-title">
        {icon}
        {title}
      </h3>
      {items.length === 0 ? (
        <p className="insights-rank-empty">{emptyHint ?? "—"}</p>
      ) : (
        <ul className="insights-rank-list">
          {items.slice(0, 8).map((r) => {
            const bits: string[] = [String(r.calls)];
            if (showTokens && r.tokens > 0) bits.push(formatTokens(r.tokens));
            if (showCost) bits.push(formatCost(r.cost_usd));
            return (
              <li key={`${r.kind}:${r.name}`} className="insights-rank-item">
                <span className="insights-rank-name">
                  <span className="insights-rank-kind-icon" title={r.kind}>
                    <RankIdentityIcon
                      type={identityType}
                      name={identityType === "kind" ? r.kind : r.name}
                    />
                  </span>
                  {r.name}
                </span>
                <span className="insights-rank-meta">{bits.join(" · ")}</span>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
