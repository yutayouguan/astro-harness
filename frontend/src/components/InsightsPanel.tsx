/** 用量洞察面板：KPI、趋势柱状图与排行；协作 Tab：编排列表 + SVG 图。 */
import { useEffect, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  AlertTriangle,
  BarChart3,
  Bot,
  Calendar,
  CalendarDays,
  CalendarRange,
  Coins,
  Cpu,
  DollarSign,
  GitBranch,
  Layers,
  MousePointerClick,
  Network,
  Plug,
  Puzzle,
  Timer,
  Wrench,
} from "lucide-react";
import { useI18n } from "../i18n/LocaleContext";
import type { Locale, MessageKey } from "../i18n/messages";
import { useAgentsChanged } from "../lib/agentsChanged";
import type { AgentInfo } from "../types/agent";
import { normalizeAgentId } from "../types/agent";
import AgentPicker from "./AgentPicker";

type Period = "month" | "quarter" | "year";
type Metric = "calls" | "tokens" | "cost";
type ViewMode = "usage" | "collab";

type UsageInsights = {
  kpis: { calls: number; tokens: number; cost_usd: number; active_agents: number };
  series: { bucket: string; calls: number; tokens: number; cost_usd: number }[];
  rankings: {
    by_kind: { kind: string; name: string; calls: number; tokens: number; cost_usd: number }[];
    by_agent: { kind: string; name: string; calls: number; tokens: number; cost_usd: number }[];
    by_model: { kind: string; name: string; calls: number; tokens: number; cost_usd: number }[];
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

const VIEW_TABS: { id: ViewMode; labelKey: MessageKey; Icon: typeof BarChart3 }[] = [
  { id: "usage", labelKey: "insights.view.usage", Icon: BarChart3 },
  { id: "collab", labelKey: "insights.view.collab", Icon: Network },
];

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
      return <Plug {...props} />;
    case "cron":
      return <Timer {...props} />;
    case "llm":
      return <Cpu {...props} />;
    case "agent":
      return <Bot {...props} />;
    default:
      return <Layers {...props} />;
  }
}

function CollabGraphSvg({
  nodes,
  edges,
}: {
  nodes: CollaborationNode[];
  edges: CollaborationEdge[];
}) {
  const w = 320;
  const h = 240;
  const cx = w / 2;
  const cy = h / 2;
  const R = 80;
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
        const label = n.label.length > 8 ? `${n.label.slice(0, 7)}…` : n.label;
        return (
          <g key={n.id}>
            <circle
              cx={p.x}
              cy={p.y}
              r={18}
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

export default function InsightsPanel({ active }: { active: boolean }) {
  const { t, locale } = useI18n();
  const [view, setView] = useState<ViewMode>("usage");
  const [period, setPeriod] = useState<Period>("month");
  const [metric, setMetric] = useState<Metric>("calls");
  const [agentId, setAgentId] = useState("workspace");
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [data, setData] = useState<UsageInsights | null>(null);
  const [collab, setCollab] = useState<CollaborationInsights | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!active || !isTauri()) return;
    void (async () => {
      try {
        const cfg = await invoke<{
          active_agent_id: string;
          agents: AgentInfo[];
        }>("get_config");
        setAgents(cfg.agents);
        setAgentId(normalizeAgentId(cfg.active_agent_id));
      } catch {
        // ignore
      }
    })();
  }, [active]);

  useAgentsChanged((payload) => {
    if (!active || !isTauri()) return;
    void (async () => {
      try {
        const cfg = await invoke<{
          active_agent_id: string;
          agents: AgentInfo[];
        }>("get_config");
        setAgents(cfg.agents);
        setAgentId(normalizeAgentId(cfg.active_agent_id || payload.active_agent_id));
      } catch {
        // ignore
      }
    })();
  });

  const switchAgent = async (id: string) => {
    setAgentId(id);
    if (!isTauri()) return;
    try {
      const cfg = await invoke<{
        active_agent_id: string;
        agents: AgentInfo[];
      }>("set_active_agent", { agentId: id });
      setAgents(cfg.agents);
      setAgentId(normalizeAgentId(cfg.active_agent_id));
    } catch {
      // keep local selection
    }
  };

  useEffect(() => {
    if (!active || !isTauri() || view !== "usage") return;
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

  const maxVal = Math.max(
    1,
    ...(data?.series.map((s) => seriesValue(s, metric)) ?? [1]),
  );
  const empty = data && data.kpis.calls === 0 && data.kpis.tokens === 0;
  const hasUnpriced = (data?.unpriced_llm_events ?? 0) > 0;
  const selected =
    collab?.orchestrations.find((o) => o.id === selectedId) ?? null;

  return (
    <div className="insights-panel">
      <div className="panel-agent-toolbar">
        <div className="panel-agent-toolbar-start">
          <AgentPicker
            agents={agents}
            value={agentId}
            onChange={(id) => void switchAgent(id)}
            labelKey="filespace.agentFilter"
          />
          <div className="insights-view-tabs" role="tablist" aria-label="insights view">
            {VIEW_TABS.map(({ id, labelKey, Icon }) => (
              <button
                key={id}
                type="button"
                role="tab"
                className={`insights-view-tab${view === id ? " active" : ""}`}
                aria-selected={view === id}
                onClick={() => setView(id)}
              >
                <Icon size={15} strokeWidth={2.25} aria-hidden />
                {t(labelKey)}
              </button>
            ))}
          </div>
        </div>
        <div className="panel-agent-toolbar-end">
          <div className="insights-period-tabs" role="tablist">
            {PERIOD_TABS.map(({ id, labelKey, Icon }) => (
              <button
                key={id}
                type="button"
                role="tab"
                className={`insights-period-tab${period === id ? " active" : ""}`}
                aria-selected={period === id}
                onClick={() => setPeriod(id)}
              >
                <Icon size={15} strokeWidth={2.25} aria-hidden />
                {t(labelKey)}
              </button>
            ))}
          </div>
        </div>
      </div>

      {error && <p className="insights-error">{error}</p>}

      {view === "usage" && data && (
        <>
          <div className="insights-kpis">
            <KpiCard
              icon={<MousePointerClick size={16} strokeWidth={2.25} aria-hidden />}
              label={t("insights.kpi.calls")}
              value={String(data.kpis.calls)}
            />
            <KpiCard
              icon={<Coins size={16} strokeWidth={2.25} aria-hidden />}
              label={t("insights.kpi.tokens")}
              value={formatTokens(data.kpis.tokens)}
            />
            <KpiCard
              icon={<DollarSign size={16} strokeWidth={2.25} aria-hidden />}
              label={t("insights.kpi.cost")}
              value={formatCost(data.kpis.cost_usd)}
            />
            <KpiCard
              icon={<Bot size={16} strokeWidth={2.25} aria-hidden />}
              label={t("insights.kpi.agents")}
              value={String(data.kpis.active_agents)}
            />
          </div>

          {hasUnpriced && (
            <p className="insights-unpriced">
              <AlertTriangle size={14} strokeWidth={2.25} aria-hidden />
              {t("insights.unpriced")}
            </p>
          )}

          {!empty && data.series.length > 0 && (
            <div className="insights-chart-wrap">
              <div className="insights-chart-heading">
                <div className="insights-chart-heading-label">
                  <BarChart3 size={15} strokeWidth={2.25} aria-hidden />
                  <span>{t(`insights.metric.${metric}`)}</span>
                </div>
                <div className="insights-metric-tabs" role="tablist">
                  {METRIC_TABS.map(({ id, labelKey }) => (
                    <button
                      key={id}
                      type="button"
                      role="tab"
                      className={`insights-metric-tab${metric === id ? " active" : ""}`}
                      aria-selected={metric === id}
                      onClick={() => setMetric(id)}
                    >
                      {t(labelKey)}
                    </button>
                  ))}
                </div>
              </div>
              <div className="insights-chart" aria-label={`${metric} trend`}>
                {data.series.map((s) => (
                  <div
                    key={s.bucket}
                    className="insights-bar-col"
                    title={formatSeriesTip(s, metric)}
                  >
                    <div
                      className="insights-bar"
                      style={{
                        height: `${(seriesValue(s, metric) / maxVal) * 100}%`,
                      }}
                    />
                    <span className="insights-bar-label">
                      {formatBucketLabel(s.bucket, period, locale)}
                    </span>
                  </div>
                ))}
              </div>
            </div>
          )}

          {empty ? (
            <div className="insights-empty">
              <span className="insights-empty-icon" aria-hidden>
                <BarChart3 size={28} strokeWidth={1.75} />
              </span>
              <p>{t("insights.empty")}</p>
            </div>
          ) : (
            <div className="insights-ranks">
              <RankList
                title={t("insights.rank.kind")}
                icon={<Layers size={14} strokeWidth={2.25} aria-hidden />}
                items={data.rankings.by_kind}
              />
              <RankList
                title={t("insights.rank.agent")}
                icon={<Bot size={14} strokeWidth={2.25} aria-hidden />}
                items={data.rankings.by_agent}
              />
              <RankList
                title={t("insights.rank.model")}
                icon={<Cpu size={14} strokeWidth={2.25} aria-hidden />}
                items={data.rankings.by_model}
                showCost
              />
            </div>
          )}
        </>
      )}

      {view === "collab" && collab && (
        collab.orchestrations.length === 0 &&
        collab.graph.nodes.length === 0 &&
        collab.graph.edges.length === 0 ? (
          <div className="insights-empty">
            <span className="insights-empty-icon" aria-hidden>
              <GitBranch size={28} strokeWidth={1.75} />
            </span>
            <p>{t("insights.collab.empty")}</p>
          </div>
        ) : (
          <div className="insights-collab-layout">
            <div className="insights-collab-left">
              <section className="insights-collab-list-panel">
                <h3 className="insights-collab-section-title">
                  <GitBranch size={14} strokeWidth={2.25} aria-hidden />
                  {t("insights.collab.listTitle")}
                </h3>
                {collab.orchestrations.length === 0 ? (
                  <p className="insights-collab-hint">{t("insights.collab.listEmpty")}</p>
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
                <p className="insights-collab-hint">{t("insights.collab.empty")}</p>
              ) : (
                <CollabGraphSvg
                  nodes={collab.graph.nodes}
                  edges={collab.graph.edges}
                />
              )}
            </section>
          </div>
        )
      )}
    </div>
  );
}

function KpiCard({
  icon,
  label,
  value,
}: {
  icon: ReactNode;
  label: string;
  value: string;
}) {
  return (
    <div className="insights-kpi">
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
}: {
  title: string;
  icon: ReactNode;
  items: { kind: string; name: string; calls: number; cost_usd: number }[];
  showCost?: boolean;
}) {
  if (items.length === 0) return null;
  return (
    <section className="insights-rank">
      <h3 className="insights-rank-title">
        {icon}
        {title}
      </h3>
      <ul className="insights-rank-list">
        {items.slice(0, 8).map((r) => (
          <li key={`${r.kind}:${r.name}`} className="insights-rank-item">
            <span className="insights-rank-name">
              <span className="insights-rank-kind-icon" title={r.kind}>
                <KindIcon kind={r.kind} />
              </span>
              {r.name}
            </span>
            <span className="insights-rank-meta">
              {r.calls}
              {showCost ? ` · ${formatCost(r.cost_usd)}` : ""}
            </span>
          </li>
        ))}
      </ul>
    </section>
  );
}
