import { BarChart3, LineChart } from "lucide-react";
import { useMemo, useState } from "react";
import { useI18n } from "../../i18n/LocaleContext";
import { ModelBrandIcon } from "../icons/ProviderIcons";

type Metric = "tokens" | "cost";
type ChartType = "bar" | "line";
type Granularity = "day" | "week" | "month";

type SeriesPoint = {
  bucket: string;
  bucket_start: string;
  bucket_end: string;
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
  granularity: Granularity;
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
  const dateOnly = /^\d{4}-\d{2}-\d{2}$/.test(value);
  const date = new Date(dateOnly ? `${value}T00:00:00Z` : value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat(locale === "zh" ? "zh-CN" : "en-US", {
    year: "numeric",
    month: "short",
    day: "numeric",
    ...(dateOnly ? { timeZone: "UTC" } : {}),
    ...(withTime ? { hour: "2-digit", minute: "2-digit", hour12: false } : {}),
  }).format(date);
}

function parseUtcDate(value: string): Date | null {
  const date = new Date(`${value}T00:00:00Z`);
  return Number.isNaN(date.getTime()) ? null : date;
}

function formatBucketRange(point: SeriesPoint, locale: string): string {
  const start = parseUtcDate(point.bucket_start);
  const endExclusive = parseUtcDate(point.bucket_end);
  if (!start || !endExclusive) return point.bucket;
  const end = new Date(endExclusive);
  end.setUTCDate(end.getUTCDate() - 1);
  if (point.bucket_start === end.toISOString().slice(0, 10)) {
    return formatDate(point.bucket_start, locale);
  }
  const formatter = new Intl.DateTimeFormat(
    locale === "zh" ? "zh-CN" : "en-US",
    { month: "short", day: "numeric", timeZone: "UTC" },
  );
  return `${formatter.format(start)} – ${formatter.format(end)}`;
}

function formatAxisLabel(
  point: SeriesPoint,
  granularity: Granularity,
  locale: string,
  index: number,
  total: number,
): string {
  const start = parseUtcDate(point.bucket_start);
  if (!start) return point.bucket;
  if (granularity === "month") {
    return new Intl.DateTimeFormat(locale === "zh" ? "zh-CN" : "en-US", {
      month: "short",
      ...(total > 12 && (index === 0 || index === total - 1)
        ? { year: "numeric" }
        : {}),
      timeZone: "UTC",
    }).format(start);
  }
  return `${String(start.getUTCMonth() + 1).padStart(2, "0")}/${String(start.getUTCDate()).padStart(2, "0")}`;
}

function shouldShowAxisLabel(
  index: number,
  total: number,
  granularity: Granularity,
): boolean {
  if (total <= 10) return true;
  const targetLabels = granularity === "day" ? 6 : 7;
  const stride = Math.max(1, Math.ceil((total - 1) / targetLabels));
  return index === 0 || index === total - 1 || index % stride === 0;
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
  const activeBuckets = data.series.filter(
    (point) => point.calls > 0 || point.tokens > 0,
  );
  const peak = activeBuckets.reduce<SeriesPoint | null>(
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
  const activeAverage = activeBuckets.length
    ? data.kpis.tokens / activeBuckets.length
    : 0;
  const max = Math.max(
    1,
    ...data.series.map((point) => metricValue(point, metric)),
  );
  const lineCoordinates = data.series.map((point, index) => {
    const x =
      data.series.length <= 1 ? 50 : (index / (data.series.length - 1)) * 1000;
    return {
      point,
      x,
      y: 205 - (metricValue(point, metric) / max) * 185,
    };
  });
  const linePoints = lineCoordinates.map(({ x, y }) => `${x},${y}`).join(" ");
  const chartEmpty = data.series.every(
    (point) => metricValue(point, metric) <= 0,
  );
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
        active: {
          day: "活跃天数",
          week: "活跃周数",
          month: "活跃月数",
        }[data.granularity],
        peak: {
          day: "高峰日",
          week: "高峰周",
          month: "高峰月",
        }[data.granularity],
        topModel: "用量最高模型",
        average: {
          day: "活跃日均",
          week: "活跃周均",
          month: "活跃月均",
        }[data.granularity],
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
        emptyChart:
          metric === "tokens"
            ? "当前范围暂无 Token 用量"
            : "当前范围暂无可估算费用",
        unpriced: "次请求暂无定价",
      }
    : {
        overview: "Overview",
        cost: "Total cost",
        requests: "Requests",
        tokens: "Total tokens",
        cache: "Cache hit rate",
        cacheRead: "Cache read",
        active: {
          day: "Active days",
          week: "Active weeks",
          month: "Active months",
        }[data.granularity],
        peak: {
          day: "Peak day",
          week: "Peak week",
          month: "Peak month",
        }[data.granularity],
        topModel: "Top model",
        average: {
          day: "Active-day average",
          week: "Active-week average",
          month: "Active-month average",
        }[data.granularity],
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
        emptyChart:
          metric === "tokens"
            ? "No token usage in this range"
            : "No estimated cost in this range",
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
    [labels.active, String(activeBuckets.length), ""],
    [
      labels.peak,
      peak ? formatTokens(peak.tokens, locale) : "—",
      peak ? formatBucketRange(peak, locale) : "",
    ],
    [
      labels.topModel,
      topModel?.name || "—",
      topModel ? formatTokens(topModel.tokens, locale) : "",
    ],
    [
      labels.average,
      formatTokens(activeAverage, locale),
      `${data.kpis.llm_calls} ${labels.requests.toLowerCase()}`,
    ],
  ] as const;

  return (
    <div className="usage-dashboard">
      <section className="usage-overview">
        <div className="usage-overview-heading">
          <h2>{labels.overview}</h2>
          <p>
            {activeBuckets.length} {labels.active.toLowerCase()} /{" "}
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
        <div
          className="usage-chart"
          data-chart={chartType}
          data-granularity={data.granularity}
        >
          {chartEmpty ? (
            <div className="usage-chart-empty-state" role="status">
              <BarChart3 size={22} strokeWidth={1.8} aria-hidden />
              <strong>{labels.emptyChart}</strong>
            </div>
          ) : chartType === "bar" ? (
            <div className="usage-bars">
              {data.series.map((point, index) => (
                <div
                  className="usage-bar-column"
                  key={point.bucket}
                  title={`${formatBucketRange(point, locale)} · ${formatTokens(metricValue(point, metric), locale)}`}
                  tabIndex={0}
                  aria-label={`${formatBucketRange(point, locale)} · ${formatTokens(metricValue(point, metric), locale)}`}
                >
                  <i
                    style={{
                      height: `${Math.max(2, (metricValue(point, metric) / max) * 100)}%`,
                    }}
                  />
                  <span aria-hidden>
                    {shouldShowAxisLabel(
                      index,
                      data.series.length,
                      data.granularity,
                    )
                      ? formatAxisLabel(
                          point,
                          data.granularity,
                          locale,
                          index,
                          data.series.length,
                        )
                      : ""}
                  </span>
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
              {lineCoordinates.map(({ point, x, y }) => (
                <circle
                  key={point.bucket}
                  cx={x}
                  cy={y}
                  r={5}
                  tabIndex={0}
                  aria-label={`${formatBucketRange(point, locale)} · ${formatTokens(metricValue(point, metric), locale)}`}
                >
                  <title>{`${formatBucketRange(point, locale)} · ${formatTokens(metricValue(point, metric), locale)}`}</title>
                </circle>
              ))}
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
