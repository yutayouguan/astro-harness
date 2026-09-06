// apps/desktop/src/lib/insightsView.ts
export type InsightsViewMode = "overview" | "models" | "tools" | "tracing";
export type InsightsMetric = "calls" | "tokens" | "cost";
export type UsageBucketState = "past" | "current" | "future";
export type UsagePeriod =
  | "month"
  | "quarter"
  | "year"
  | "days30"
  | "days90"
  | "days365";

export const DEFAULT_INSIGHTS_VIEW: InsightsViewMode = "overview";

export const INSIGHTS_VIEW_ORDER: readonly InsightsViewMode[] = [
  "overview",
  "models",
  "tools",
  "tracing",
] as const;

export function needsUsageInsights(view: InsightsViewMode): boolean {
  return view === "overview" || view === "models" || view === "tools";
}

export type InsightsRankItem = {
  kind: string;
  name: string;
  calls: number;
  tokens: number;
  cost_usd: number;
};

export function rankValue(
  item: InsightsRankItem,
  metric: InsightsMetric,
): number {
  if (metric === "tokens") return item.tokens;
  if (metric === "cost") return item.cost_usd;
  return item.calls;
}

/** 排行与当前展示指标保持一致，并用其他用量字段稳定破平。 */
export function rankByMetric(
  items: InsightsRankItem[],
  metric: InsightsMetric,
  n = 5,
): InsightsRankItem[] {
  return [...items]
    .sort(
      (a, b) =>
        rankValue(b, metric) - rankValue(a, metric) ||
        b.tokens - a.tokens ||
        b.calls - a.calls ||
        a.name.localeCompare(b.name),
    )
    .slice(0, Math.max(0, n));
}

export function costCoveragePercent(
  llmCalls: number,
  unpricedLlmEvents: number,
): number | null {
  if (llmCalls <= 0) return null;
  const priced = Math.max(0, llmCalls - Math.max(0, unpricedLlmEvents));
  return Math.round((Math.min(llmCalls, priced) / llmCalls) * 100);
}

export function usageBucketState(
  bucket: string,
  period: UsagePeriod,
  asOfIso = new Date().toISOString(),
): UsageBucketState {
  const currentBucket =
    period === "month" || period.startsWith("days")
      ? asOfIso.slice(0, 10)
      : asOfIso.slice(0, 7);
  if (bucket === currentBucket) return "current";
  return bucket > currentBucket ? "future" : "past";
}

const PROVIDER_LABELS: Record<string, string> = {
  openai: "OpenAI",
  deepseek: "DeepSeek",
  google: "Google",
  anthropic: "Anthropic",
  openrouter: "OpenRouter",
  azure: "Azure OpenAI",
  bailian: "Bailian",
  zhipu: "Zhipu AI",
  moonshot: "Moonshot",
  minimax: "MiniMax",
  meta: "Meta",
  volcengine: "Volcengine",
  ollama: "Ollama",
  mimo: "MiMo",
};

export function providerDisplayName(
  name: string,
  otherLabel = "Other",
  unknownLabel = "Unknown",
): string {
  const key = name.trim().toLowerCase();
  if (key === "other") return otherLabel;
  if (key === "unknown" || !key) return unknownLabel;
  return PROVIDER_LABELS[key] ?? name;
}

/**
 * 从模型 ID 推断厂商。
 *
 * - Google API 资源路径：`models/gemini-…` → `google`（勿把 `models` 当厂商）
 * - OpenRouter 等：`vendor/model` → `vendor`
 * - 裸名：按常见前缀启发式
 */
export function inferProvider(modelName: string): string {
  let n = modelName.trim().toLowerCase();
  if (!n) return "unknown";

  // Google Generative Language API: "models/gemini-3.5-flash"
  if (n.startsWith("models/")) {
    n = n.slice("models/".length);
  }

  if (n.includes("/")) {
    return n.split("/")[0] || "unknown";
  }

  if (
    n.startsWith("gpt") ||
    n.startsWith("o1") ||
    n.startsWith("o3") ||
    n.startsWith("o4") ||
    n.startsWith("chatgpt")
  ) {
    return "openai";
  }
  if (n.startsWith("claude")) return "anthropic";
  if (n.startsWith("gemini")) return "google";
  if (n.startsWith("deepseek")) return "deepseek";
  if (n.startsWith("qwen")) return "bailian";
  if (n.startsWith("glm") || n.startsWith("chatglm")) return "zhipu";
  if (n.startsWith("kimi") || n.startsWith("moonshot")) return "moonshot";
  if (n.startsWith("minimax")) return "minimax";
  if (n.startsWith("mimo")) return "mimo";
  if (n.startsWith("llama")) return "meta";
  if (n.startsWith("ep-")) return "volcengine";
  return "other";
}

/** 将按模型排行聚合为按厂商排行。 */
export function aggregateByProvider(
  models: InsightsRankItem[],
): InsightsRankItem[] {
  const map = new Map<string, InsightsRankItem>();
  for (const m of models) {
    const provider = inferProvider(m.name);
    const cur = map.get(provider);
    if (cur) {
      cur.calls += m.calls;
      cur.tokens += m.tokens;
      cur.cost_usd += m.cost_usd;
    } else {
      map.set(provider, {
        kind: "provider",
        name: provider,
        calls: m.calls,
        tokens: m.tokens,
        cost_usd: m.cost_usd,
      });
    }
  }
  return [...map.values()].sort(
    (a, b) =>
      b.cost_usd - a.cost_usd || b.tokens - a.tokens || b.calls - a.calls,
  );
}
