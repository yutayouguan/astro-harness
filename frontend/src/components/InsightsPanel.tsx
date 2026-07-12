/** 用量洞察面板：KPI、趋势柱状图与排行。 */
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
  Layers,
  MousePointerClick,
  Plug,
  Puzzle,
  Timer,
  Wrench,
} from "lucide-react";
import { useI18n } from "../i18n/LocaleContext";
import type { MessageKey } from "../i18n/messages";
import { useAgentsChanged } from "../lib/agentsChanged";
import type { AgentInfo } from "../types/agent";
import { normalizeAgentId } from "../types/agent";
import AgentPicker from "./AgentPicker";

type Period = "month" | "quarter" | "year";

type UsageInsights = {
  kpis: { calls: number; tokens: number; cost_usd: number; active_agents: number };
  series: { bucket: string; calls: number; tokens: number; cost_usd: number }[];
  rankings: {
    by_kind: { kind: string; name: string; calls: number; tokens: number; cost_usd: number }[];
    by_agent: { kind: string; name: string; calls: number; tokens: number; cost_usd: number }[];
    by_model: { kind: string; name: string; calls: number; tokens: number; cost_usd: number }[];
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

export default function InsightsPanel({ active }: { active: boolean }) {
  const { t } = useI18n();
  const [period, setPeriod] = useState<Period>("month");
  const [agentId, setAgentId] = useState("workspace");
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [data, setData] = useState<UsageInsights | null>(null);
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
    if (!active || !isTauri()) return;
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
  }, [active, period, agentId]);

  const maxCalls = Math.max(1, ...(data?.series.map((s) => s.calls) ?? [1]));
  const empty = data && data.kpis.calls === 0 && data.kpis.tokens === 0;
  const hasUnpriced =
    data &&
    data.rankings.by_model.some((m) => m.calls > 0 && m.cost_usd <= 0);

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

      {data && (
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
                <BarChart3 size={15} strokeWidth={2.25} aria-hidden />
                <span>{t("insights.kpi.calls")}</span>
              </div>
              <div className="insights-chart" aria-label="calls trend">
                {data.series.map((s) => (
                  <div
                    key={s.bucket}
                    className="insights-bar-col"
                    title={`${s.bucket}: ${s.calls}`}
                  >
                    <div
                      className="insights-bar"
                      style={{ height: `${(s.calls / maxCalls) * 100}%` }}
                    />
                    <span className="insights-bar-label">{s.bucket.slice(-5)}</span>
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
