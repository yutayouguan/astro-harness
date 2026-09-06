/** 用量洞察面板：模型用量 / 工具技能 / Tracing 分 Tab。 */
import { useEffect, useMemo, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Activity,
  BarChart3,
  Bot,
  Building2,
  Calendar,
  CalendarDays,
  CalendarRange,
  Cpu,
  GitBranch,
  Layers,
  LayoutDashboard,
  Puzzle,
  Timer,
  UserRound,
  Wrench,
} from "lucide-react";
import {
  ChevronDown as ChevronDownData,
  ChevronRight as ChevronRightData,
} from "lucide";
import { useActiveAgent } from "../../hooks/app/useActiveAgent";
import { useI18n } from "../../i18n/LocaleContext";
import type { Locale, MessageKey } from "../../i18n/messages";
import {
  aggregateByProvider,
  DEFAULT_INSIGHTS_VIEW,
  INSIGHTS_VIEW_ORDER,
  needsUsageInsights,
  providerDisplayName,
  usageBucketState,
  type InsightsViewMode,
} from "../../lib/insights/insightsView";
import {
  groupEventsForTraceDisplay,
  shortTurnId,
  traceSessionTitle,
  turnGroupTitle,
} from "../../lib/chat/traceTurnGroups";
import McpIcon from "../icons/McpIcon";
import { MorphToggleIcon } from "../icons/MorphIcon";
import { ModelBrandIcon, ProviderBrandIcon } from "../icons/ProviderIcons";
import { SegmentedTabs } from "../ui";
import UsageDashboard from "./UsageDashboard";

type Period = "days30" | "days90" | "days365";
type ViewMode = InsightsViewMode;

type RankItem = {
  kind: string;
  name: string;
  calls: number;
  tokens: number;
  cost_usd: number;
};

export type UsageInsights = {
  kpis: {
    calls: number;
    tokens: number;
    cost_usd: number;
    active_agents: number;
    llm_calls: number;
    input_tokens: number;
    output_tokens: number;
    cache_tokens: number;
    cache_read_tokens?: number;
    cache_write_tokens?: number;
    reasoning_tokens: number;
  };
  series: { bucket: string; calls: number; tokens: number; cost_usd: number }[];
  rankings: {
    by_kind: RankItem[];
    by_agent: RankItem[];
    by_model: RankItem[];
  };
  recent_requests?: Array<{
    id: string;
    ts: string;
    model: string;
    provider?: string | null;
    agent_id: string;
    input_tokens: number;
    output_tokens: number;
    cache_read_tokens: number;
    total_tokens: number;
    cost_usd: number;
  }>;
  unpriced_llm_events?: number;
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
  { id: "days30", labelKey: "insights.period.days30", Icon: Calendar },
  { id: "days90", labelKey: "insights.period.days90", Icon: CalendarRange },
  { id: "days365", labelKey: "insights.period.days365", Icon: CalendarDays },
];

const VIEW_TAB_META: Record<
  ViewMode,
  { labelKey: MessageKey; Icon: typeof BarChart3 }
> = {
  overview: { labelKey: "insights.view.overview", Icon: LayoutDashboard },
  models: { labelKey: "insights.view.models", Icon: Cpu },
  tools: { labelKey: "insights.view.tools", Icon: Wrench },
  tracing: { labelKey: "insights.view.tracing", Icon: Activity },
};

const VIEW_TABS = INSIGHTS_VIEW_ORDER.map((id) => ({
  id,
  ...VIEW_TAB_META[id],
}));

function isGenericProvider(name: string): boolean {
  const normalized = name.trim().toLowerCase();
  return normalized === "other" || normalized === "unknown" || !normalized;
}

/** 柱高用像素，避免百分比在 flex 里塌成贴底细线；非零值保底可见。 */
const CHART_PLOT_H = 176;
function barHeightPx(value: number, maxVal: number): number {
  if (value <= 0 || maxVal <= 0) return 0;
  const raw = (value / maxVal) * CHART_PLOT_H;
  return Math.max(10, Math.round(raw));
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

/** 按 period / locale 格式化桶标签。滚动周期均由后端返回每日桶。 */
function formatBucketLabel(
  bucket: string,
  period: Period,
  locale: Locale,
): string {
  if (period.startsWith("days")) {
    const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(bucket);
    if (m) {
      const month = Number(m[2]);
      const day = Number(m[3]);
      return locale === "zh" ? `${day}日` : `${month}/${day}`;
    }
  }
  return bucket;
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
    if (isGenericProvider(name)) {
      return <Building2 size={14} strokeWidth={2.1} aria-hidden />;
    }
    return <ProviderBrandIcon kind={name} size={14} />;
  }
  return <KindIcon kind={name} />;
}

export default function InsightsPanel({
  active,
  initialData,
}: {
  active: boolean;
  initialData?: UsageInsights;
}) {
  const { t, locale } = useI18n();
  const { activeAgentId: agentId } = useActiveAgent();
  const [view, setView] = useState<ViewMode>(DEFAULT_INSIGHTS_VIEW);
  const [period, setPeriod] = useState<Period>("days30");
  const [data, setData] = useState<UsageInsights | null>(initialData ?? null);
  const [annualSeries, setAnnualSeries] = useState<UsageInsights["series"]>(
    initialData?.series ?? [],
  );
  const [traces, setTraces] = useState<TraceInsights | null>(null);
  const [selectedTraceId, setSelectedTraceId] = useState<string | null>(null);
  const [expandedTurns, setExpandedTurns] = useState<Record<string, boolean>>(
    {},
  );
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (initialData) {
      setData(initialData);
      setAnnualSeries(initialData.series);
      return;
    }
    if (!active || !isTauri() || !needsUsageInsights(view)) return;
    let cancelled = false;
    void (async () => {
      try {
        const args = { as_of: null, agent_id: agentId };
        const [res, annual] = await Promise.all([
          invoke<UsageInsights>("get_usage_insights", {
            args: { ...args, period },
          }),
          view !== "overview" || period === "days365"
            ? Promise.resolve(null)
            : invoke<UsageInsights>("get_usage_insights", {
                args: { ...args, period: "days365" },
              }),
        ]);
        if (!cancelled) {
          setData(res);
          setAnnualSeries(annual?.series ?? res.series);
          setError(null);
        }
      } catch (e) {
        if (!cancelled) setError(String(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [active, period, agentId, initialData, view]);

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
            if (prev && res.traces.some((t) => t.session_id === prev))
              return prev;
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
  const providerName = (name: string) =>
    providerDisplayName(
      name,
      t("insights.provider.other"),
      t("insights.provider.unknown"),
    );

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
        <SegmentedTabs
          className="insights-view-tabs"
          aria-label="insights view"
          value={view}
          onValueChange={(value) => setView(value as ViewMode)}
          items={VIEW_TABS.map(({ id, labelKey, Icon }) => ({
            value: id,
            label: t(labelKey),
            icon: <Icon size={15} strokeWidth={2.25} />,
            className: "insights-view-tab",
          }))}
        />
        <SegmentedTabs
          className="insights-period-tabs"
          aria-label="insights period"
          value={period}
          onValueChange={(value) => setPeriod(value as Period)}
          items={PERIOD_TABS.map(({ id, labelKey, Icon }) => ({
            value: id,
            label: t(labelKey),
            icon: <Icon size={15} strokeWidth={2.25} />,
            className: "insights-period-tab",
          }))}
        />
      </div>

      <div className="insights-body">
        {error && <p className="insights-error">{error}</p>}

        {view === "overview" && data && (
          <UsageDashboard data={data} annualSeries={annualSeries} />
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
                <p className="insights-rank-empty">
                  {t("insights.rank.empty")}
                </p>
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
                formatName={providerName}
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
              <p className="insights-panel-hint">
                {t("insights.empty.modelsHint")}
              </p>
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
                  {data.series.map((s) => {
                    const bucketState = usageBucketState(s.bucket, period);
                    const bucketLabel = formatBucketLabel(
                      s.bucket,
                      period,
                      locale,
                    );
                    const tip = `${bucketLabel}: ${s.calls}`;
                    return (
                      <div
                        key={s.bucket}
                        className={`insights-bar-col is-${bucketState}`}
                        title={bucketState === "future" ? undefined : tip}
                        aria-label={bucketState === "future" ? undefined : tip}
                        aria-hidden={
                          bucketState === "future" ? true : undefined
                        }
                        tabIndex={bucketState === "future" ? -1 : 0}
                      >
                        <div
                          className="insights-bar-plot"
                          style={{ height: CHART_PLOT_H }}
                        >
                          <div
                            className={`insights-bar${s.calls > 0 ? "" : " is-empty"}${bucketState === "future" ? " is-future" : ""}`}
                            style={{
                              height: barHeightPx(s.calls, toolSeriesMax),
                            }}
                          />
                        </div>
                        <span className="insights-bar-label">
                          {bucketLabel}
                        </span>
                        {bucketState !== "future" && (
                          <span className="insights-bar-tooltip" role="tooltip">
                            {tip}
                          </span>
                        )}
                      </div>
                    );
                  })}
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
                    <li
                      key={`${r.kind}:${r.name}`}
                      className="insights-hbar-item"
                    >
                      <span className="insights-hbar-label">
                        <span
                          className="insights-rank-kind-icon"
                          title={r.kind}
                        >
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

            {toolCallTotal + skillCallTotal + mcpCallTotal + cronCallTotal ===
              0 && (
              <p className="insights-panel-hint">
                {t("insights.empty.toolsHint")}
              </p>
            )}
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
                    <p className="insights-panel-hint">
                      {t("insights.trace.emptyHint")}
                    </p>
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
                            <span className="insights-collab-list-agent">
                              {tr.agent_id}
                            </span>
                            <span className="insights-trace-meta-chip">
                              {formatTraceTime(tr.started_at)} ·{" "}
                              {tr.event_count} · {formatTokens(tr.tokens)}
                            </span>
                          </span>
                          <span className="insights-trace-kinds">
                            {tr.kinds.map((k) => (
                              <span
                                key={k}
                                className={`insights-trace-kind kind-${k}`}
                              >
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
                  <p className="insights-collab-hint">
                    {t("insights.trace.noSelection")}
                  </p>
                ) : selectedTrace.events.length === 0 ? (
                  <p className="insights-collab-hint">
                    {t("insights.trace.listEmpty")}
                  </p>
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
                        ? shortTurnId(
                            g.turn_id,
                            t("insights.trace.unlabeledTurn"),
                          )
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
                            <span className="insights-turn-group-label">
                              {label}
                            </span>
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
  const [open, setOpen] = useState(false);
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
      <details
        className="insights-trace-event-details"
        onToggle={(e) => setOpen(e.currentTarget.open)}
      >
        <summary className="insights-trace-event-summary">
          <span className="insights-trace-event-chevron" aria-hidden>
            <MorphToggleIcon
              active={open}
              activeIcon={ChevronDownData}
              inactiveIcon={ChevronRightData}
              size={14}
              strokeWidth={2.25}
            />
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
  detail,
  actionLabel,
  onAction,
  compact = false,
  emphasis = "default",
}: {
  icon: ReactNode;
  label: string;
  value: string;
  detail?: string;
  actionLabel?: string;
  onAction?: () => void;
  compact?: boolean;
  emphasis?: "default" | "primary" | "muted" | "warning";
}) {
  return (
    <div
      className={`insights-kpi insights-kpi--${emphasis}${compact ? " insights-kpi--compact" : ""}`}
    >
      <span className="insights-kpi-label">
        <span className="insights-kpi-icon">{icon}</span>
        {label}
      </span>
      <span className="insights-kpi-value">{value}</span>
      {(detail || (actionLabel && onAction)) && (
        <span className="insights-kpi-footer">
          {detail && <span className="insights-kpi-detail">{detail}</span>}
          {actionLabel && onAction && (
            <button
              type="button"
              className="insights-kpi-action"
              onClick={onAction}
            >
              {actionLabel}
            </button>
          )}
        </span>
      )}
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
  formatName = (name) => name,
}: {
  title: string;
  icon: ReactNode;
  items: RankItem[];
  showCost?: boolean;
  showTokens?: boolean;
  emptyHint?: string;
  identityType?: "model" | "provider" | "kind";
  formatName?: (name: string) => string;
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
                  {formatName(r.name)}
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
