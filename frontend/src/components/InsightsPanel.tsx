/** 用量洞察面板：KPI、趋势柱状图与排行。 */
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "../i18n/LocaleContext";
import type { AgentInfo } from "../types/agent";
import { normalizeAgentId } from "../types/agent";

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

export default function InsightsPanel({ active }: { active: boolean }) {
  const { t } = useI18n();
  const [period, setPeriod] = useState<Period>("month");
  const [agentId, setAgentId] = useState<string | null>(null);
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [data, setData] = useState<UsageInsights | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!active || !isTauri()) return;
    void (async () => {
      try {
        const cfg = await invoke<{ agents: AgentInfo[] }>("get_config");
        setAgents(cfg.agents);
      } catch {
        // ignore
      }
    })();
  }, [active]);

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
      <div className="insights-toolbar">
        <div className="insights-period-tabs" role="tablist">
          {(["month", "quarter", "year"] as Period[]).map((p) => (
            <button
              key={p}
              type="button"
              role="tab"
              className={`insights-period-tab${period === p ? " active" : ""}`}
              aria-selected={period === p}
              onClick={() => setPeriod(p)}
            >
              {t(`insights.period.${p}`)}
            </button>
          ))}
        </div>
        <label className="insights-agent-filter">
          <span className="sr-only">{t("insights.allAgents")}</span>
          <select
            value={agentId ?? ""}
            onChange={(e) => {
              const v = e.target.value;
              setAgentId(v ? normalizeAgentId(v) : null);
            }}
          >
            <option value="">{t("insights.allAgents")}</option>
            {agents.map((a) => (
              <option key={a.id} value={a.id}>
                {a.name || a.id}
              </option>
            ))}
          </select>
        </label>
      </div>

      {error && <p className="insights-error">{error}</p>}

      {data && (
        <>
          <div className="insights-kpis">
            <div className="insights-kpi">
              <span className="insights-kpi-label">{t("insights.kpi.calls")}</span>
              <span className="insights-kpi-value">{data.kpis.calls}</span>
            </div>
            <div className="insights-kpi">
              <span className="insights-kpi-label">{t("insights.kpi.tokens")}</span>
              <span className="insights-kpi-value">
                {formatTokens(data.kpis.tokens)}
              </span>
            </div>
            <div className="insights-kpi">
              <span className="insights-kpi-label">{t("insights.kpi.cost")}</span>
              <span className="insights-kpi-value">
                {formatCost(data.kpis.cost_usd)}
              </span>
            </div>
            <div className="insights-kpi">
              <span className="insights-kpi-label">{t("insights.kpi.agents")}</span>
              <span className="insights-kpi-value">{data.kpis.active_agents}</span>
            </div>
          </div>

          {hasUnpriced && (
            <p className="insights-unpriced">{t("insights.unpriced")}</p>
          )}

          {!empty && data.series.length > 0 && (
            <div className="insights-chart" aria-label="calls trend">
              {data.series.map((s) => (
                <div key={s.bucket} className="insights-bar-col" title={`${s.bucket}: ${s.calls}`}>
                  <div
                    className="insights-bar"
                    style={{ height: `${(s.calls / maxCalls) * 100}%` }}
                  />
                  <span className="insights-bar-label">{s.bucket.slice(-5)}</span>
                </div>
              ))}
            </div>
          )}

          {empty ? (
            <p className="insights-empty">{t("insights.empty")}</p>
          ) : (
            <div className="insights-ranks">
              <RankList
                title={t("insights.rank.kind")}
                items={data.rankings.by_kind}
              />
              <RankList
                title={t("insights.rank.agent")}
                items={data.rankings.by_agent}
              />
              <RankList
                title={t("insights.rank.model")}
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

function RankList({
  title,
  items,
  showCost,
}: {
  title: string;
  items: { kind: string; name: string; calls: number; cost_usd: number }[];
  showCost?: boolean;
}) {
  if (items.length === 0) return null;
  return (
    <section className="insights-rank">
      <h3 className="insights-rank-title">{title}</h3>
      <ul className="insights-rank-list">
        {items.slice(0, 8).map((r) => (
          <li key={`${r.kind}:${r.name}`} className="insights-rank-item">
            <span className="insights-rank-name">
              {r.kind !== "agent" && r.kind !== "llm" ? (
                <em className="insights-rank-kind">{r.kind}</em>
              ) : null}
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
