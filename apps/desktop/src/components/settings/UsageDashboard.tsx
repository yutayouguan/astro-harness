import { BarChart3, LineChart } from "lucide-react";
import { useMemo, useState } from "react";
import { useI18n } from "../../i18n/LocaleContext";
import { ModelBrandIcon } from "../icons/ProviderIcons";

type Metric = "tokens" | "cost";
type ChartType = "bar" | "line";

type SeriesPoint = {
  bucket: string;
  calls: number;
  tokens: number;
  cost_usd: number;
};

type RequestRow = {
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
};

export type UsageDashboardData = {
  kpis: {
    tokens: number;
    cost_usd: number;
    llm_calls: number;
    input_tokens: number;
    cache_tokens: number;
    cache_read_tokens?: number;
  };
  series: SeriesPoint[];
  rankings: {
    by_model: Array<{
      name: string;
      calls: number;
      tokens: number;
      cost_usd: number;
    }>;
  };
  recent_requests?: RequestRow[];
  unpriced_llm_events?: number;
};

function formatTokens(value: number, locale: string): string {
  if (locale === "zh") {
    if (value >= 10_000) return `${(value / 10_000).toFixed(1)}万`;
  } else if (value >= 1_000_000) {
    return `${(value / 1_000_000).toFixed(1)}M`;
  }
  if (value >= 1_000) return `${(value / 1_000).toFixed(1)}K`;
  return String(Math.round(value));
}

function formatCost(value: number): string {
  if (value <= 0) return "$0";
  if (value < 0.0001) return "<$0.0001";
  return `$${value < 0.01 ? value.toFixed(4) : value.toFixed(2)}`;
}

function formatDate(value: string, locale: string, withTime = false): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat(locale === "zh" ? "zh-CN" : "en-US", {
    year: "numeric",
    month: "short",
    day: "numeric",
    ...(withTime ? { hour: "2-digit", minute: "2-digit", hour12: false } : {}),
  }).format(date);
}

function metricValue(point: SeriesPoint, metric: Metric): number {
  return metric === "cost" ? point.cost_usd : point.tokens;
}

function MiniSparkline({ values }: { values: number[] }) {
  const samples = values.slice(-16);
  const max = Math.max(1, ...samples);
  const points = samples
    .map((value, index) => {
      const x = samples.length <= 1 ? 50 : (index / (samples.length - 1)) * 100;
      return `${x},${30 - (value / max) * 26}`;
    })
    .join(" ");
  return (
    <svg className="usage-sparkline" viewBox="0 0 100 32" aria-hidden>
      <polyline points={points} />
    </svg>
  );
}

function UsageHeatmap({
  series,
  locale,
}: {
  series: SeriesPoint[];
  locale: string;
}) {
  const values = series.map((point) => point.tokens);
  const max = Math.max(1, ...values);
  const first = series[0]?.bucket;
  const leading = first ? new Date(`${first}T00:00:00Z`).getUTCDay() : 0;
  const months = Array.from(
    new Set(series.map((point) => point.bucket.slice(0, 7))),
  );

  return (
    <section className="usage-activity-card">
      <div className="usage-section-head">
        <h3>{locale === "zh" ? "每日活动" : "Daily activity"}</h3>
        <span>Token</span>
      </div>
      <div className="usage-heatmap-months" aria-hidden>
        {months.map((month) => (
          <span key={month}>
            {new Intl.DateTimeFormat(locale === "zh" ? "zh-CN" : "en-US", {
              month: "short",
              timeZone: "UTC",
            }).format(new Date(`${month}-01T00:00:00Z`))}
          </span>
        ))}
      </div>
      <div
        className="usage-heatmap"
        aria-label={
          locale === "zh"
            ? "每日 Token 使用热力图"
            : "Daily token usage heatmap"
        }
      >
        {Array.from({ length: leading }, (_, index) => (
          <i key={`empty-${index}`} aria-hidden />
        ))}
        {series.map((point) => {
          const level =
            point.tokens <= 0 ? 0 : Math.ceil((point.tokens / max) * 4);
          return (
            <i
              key={point.bucket}
              data-level={level}
              title={`${formatDate(point.bucket, locale)} · ${formatTokens(point.tokens, locale)} Token`}
            />
          );
        })}
      </div>
    </section>
  );
}

export default function UsageDashboard({
  data,
  annualSeries,
}: {
  data: UsageDashboardData;
  annualSeries: SeriesPoint[];
}) {
  const { locale } = useI18n();
  const zh = locale === "zh";
  const [metric, setMetric] = useState<Metric>("tokens");
  const [chartType, setChartType] = useState<ChartType>("bar");
  const activeDays = data.series.filter(
    (point) => point.calls > 0 || point.tokens > 0,
  );
  const peak = activeDays.reduce<SeriesPoint | null>(
    (best, point) => (!best || point.tokens > best.tokens ? point : best),
    null,
  );
  const topModel = [...data.rankings.by_model].sort(
    (left, right) => right.tokens - left.tokens,
  )[0];
  const cacheRead = data.kpis.cache_read_tokens ?? data.kpis.cache_tokens;
  const cacheInput = data.kpis.input_tokens + cacheRead;
  const cacheRate = cacheInput
    ? Math.min(100, (cacheRead / cacheInput) * 100)
    : 0;
  const dailyAverage = activeDays.length
    ? data.kpis.tokens / activeDays.length
    : 0;
  const max = Math.max(
    1,
    ...data.series.map((point) => metricValue(point, metric)),
  );
  const linePoints = data.series
    .map((point, index) => {
      const x =
        data.series.length <= 1
          ? 50
          : (index / (data.series.length - 1)) * 1000;
      return `${x},${205 - (metricValue(point, metric) / max) * 185}`;
    })
    .join(" ");
  const requests = useMemo(
    () => (data.recent_requests ?? []).slice(0, 50),
    [data.recent_requests],
  );
  const labels = zh
    ? {
        overview: "概览",
        cost: "总成本",
        requests: "请求数",
        tokens: "总 Token 数",
        cache: "缓存命中率",
        cacheRead: "缓存读取",
        activeDays: "活跃天数",
        peak: "高峰日",
        topModel: "用量最高模型",
        average: "日均",
        analysis: "分析",
        bar: "柱状图",
        line: "折线图",
        requestList: "请求",
        records: "条记录",
        model: "模型",
        source: "来源",
        date: "日期",
        requestCost: "成本",
        requestCache: "缓存",
        empty: "暂无请求记录",
        unpriced: "次请求暂无定价",
      }
    : {
        overview: "Overview",
        cost: "Total cost",
        requests: "Requests",
        tokens: "Total tokens",
        cache: "Cache hit rate",
        cacheRead: "Cache read",
        activeDays: "Active days",
        peak: "Peak day",
        topModel: "Top model",
        average: "Daily average",
        analysis: "Analysis",
        bar: "Bars",
        line: "Line",
        requestList: "Requests",
        records: "records",
        model: "Model",
        source: "Source",
        date: "Date",
        requestCost: "Cost",
        requestCache: "Cache",
        empty: "No request records",
        unpriced: "requests are not priced",
      };

  const kpis = [
    [
      labels.cost,
      formatCost(data.kpis.cost_usd),
      data.unpriced_llm_events
        ? `${data.unpriced_llm_events} ${labels.unpriced}`
        : "",
    ],
    [labels.requests, String(data.kpis.llm_calls), ""],
    [labels.tokens, formatTokens(data.kpis.tokens, locale), ""],
    [
      labels.cache,
      `${cacheRate.toFixed(1)}%`,
      `${labels.cacheRead}: ${formatTokens(cacheRead, locale)}`,
    ],
    [labels.activeDays, String(activeDays.length), ""],
    [
      labels.peak,
      peak ? formatTokens(peak.tokens, locale) : "—",
      peak ? formatDate(peak.bucket, locale) : "",
    ],
    [
      labels.topModel,
      topModel?.name || "—",
      topModel ? formatTokens(topModel.tokens, locale) : "",
    ],
    [
      labels.average,
      formatTokens(dailyAverage, locale),
      `${data.kpis.llm_calls} ${labels.requests.toLowerCase()}`,
    ],
  ] as const;

  return (
    <div className="usage-dashboard">
      <section className="usage-overview">
        <div className="usage-overview-heading">
          <h2>{labels.overview}</h2>
          <p>
            {activeDays.length} {labels.activeDays.toLowerCase()} /{" "}
            {formatTokens(data.kpis.tokens, locale)} Token /{" "}
            {data.kpis.llm_calls} {labels.requests.toLowerCase()}
          </p>
        </div>
        <div className="usage-kpi-grid">
          {kpis.map(([label, value, detail], index) => (
            <article className="usage-kpi-card" key={label}>
              <span>{label}</span>
              <strong className={index === 6 ? "is-model" : undefined}>
                {value}
              </strong>
              {detail ? <small>{detail}</small> : null}
              {index === 0 ? (
                <MiniSparkline
                  values={data.series.map((point) => point.cost_usd)}
                />
              ) : index === 1 ? (
                <MiniSparkline
                  values={data.series.map((point) => point.calls)}
                />
              ) : index === 2 ? (
                <MiniSparkline
                  values={data.series.map((point) => point.tokens)}
                />
              ) : null}
            </article>
          ))}
        </div>
      </section>

      <UsageHeatmap series={annualSeries} locale={locale} />

      <div className="usage-section-label">
        <h2>{labels.analysis}</h2>
        <p>{formatTokens(data.kpis.tokens, locale)} Token</p>
      </div>
      <section className="usage-analysis-card">
        <div className="usage-section-head usage-analysis-head">
          <div
            className="usage-segmented"
            role="radiogroup"
            aria-label={zh ? "统计指标" : "Metric"}
          >
            {(["tokens", "cost"] as const).map((value) => (
              <button
                key={value}
                type="button"
                className={metric === value ? "is-active" : undefined}
                aria-checked={metric === value}
                role="radio"
                onClick={() => setMetric(value)}
              >
                {value === "tokens" ? "Token" : labels.requestCost}
              </button>
            ))}
          </div>
          <div
            className="usage-segmented"
            role="radiogroup"
            aria-label={zh ? "图表类型" : "Chart type"}
          >
            <button
              type="button"
              className={chartType === "bar" ? "is-active" : undefined}
              aria-checked={chartType === "bar"}
              role="radio"
              onClick={() => setChartType("bar")}
            >
              <BarChart3 size={14} aria-hidden /> {labels.bar}
            </button>
            <button
              type="button"
              className={chartType === "line" ? "is-active" : undefined}
              aria-checked={chartType === "line"}
              role="radio"
              onClick={() => setChartType("line")}
            >
              <LineChart size={14} aria-hidden /> {labels.line}
            </button>
          </div>
        </div>
        <div className="usage-chart" data-chart={chartType}>
          {chartType === "bar" ? (
            <div className="usage-bars">
              {data.series.map((point) => (
                <div
                  className="usage-bar-column"
                  key={point.bucket}
                  title={`${formatDate(point.bucket, locale)} · ${formatTokens(metricValue(point, metric), locale)}`}
                >
                  <i
                    style={{
                      height: `${Math.max(2, (metricValue(point, metric) / max) * 100)}%`,
                    }}
                  />
                  <span>{point.bucket.slice(5).replace("-", "/")}</span>
                </div>
              ))}
            </div>
          ) : (
            <svg
              viewBox="0 0 1000 220"
              preserveAspectRatio="none"
              aria-label={zh ? "用量折线图" : "Usage line chart"}
            >
              <polyline points={linePoints} />
            </svg>
          )}
        </div>
      </section>

      <section className="usage-requests-card">
        <div className="usage-section-head">
          <h3>{labels.requestList}</h3>
          <span>
            {data.kpis.llm_calls} {labels.records}
          </span>
        </div>
        <div className="usage-table-wrap">
          <table className="usage-request-table">
            <thead>
              <tr>
                <th>{labels.model}</th>
                <th>{labels.source}</th>
                <th>{labels.date}</th>
                <th>Token</th>
                <th>{labels.requestCost}</th>
                <th>{labels.requestCache}</th>
              </tr>
            </thead>
            <tbody>
              {requests.map((request) => (
                <tr key={request.id}>
                  <td>
                    <span className="usage-model-cell">
                      <ModelBrandIcon modelId={request.model} size={18} />
                      <span>
                        <strong>{request.model}</strong>
                        <small>{request.provider || "—"}</small>
                      </span>
                    </span>
                  </td>
                  <td>{request.agent_id || "—"}</td>
                  <td>{formatDate(request.ts, locale, true)}</td>
                  <td>{formatTokens(request.total_tokens, locale)}</td>
                  <td>{formatCost(request.cost_usd)}</td>
                  <td>{formatTokens(request.cache_read_tokens, locale)}</td>
                </tr>
              ))}
              {requests.length === 0 ? (
                <tr>
                  <td className="usage-table-empty" colSpan={6}>
                    {labels.empty}
                  </td>
                </tr>
              ) : null}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  );
}
