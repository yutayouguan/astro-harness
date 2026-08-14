// apps/desktop/src/lib/insightsView.ts
export type InsightsViewMode =
  | "overview"
  | "models"
  | "tools"
  | "collab"
  | "tracing"
  | "api";

export const DEFAULT_INSIGHTS_VIEW: InsightsViewMode = "overview";

export const INSIGHTS_VIEW_ORDER: readonly InsightsViewMode[] = [
  "overview",
  "models",
  "tools",
  "collab",
  "tracing",
  "api",
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

/** 按费用（估）降序，同费用再按 tokens；截断为 Top N。 */
export function providerSpendTop(
  items: InsightsRankItem[],
  n = 5,
): InsightsRankItem[] {
  return [...items]
    .sort(
      (a, b) =>
        b.cost_usd - a.cost_usd ||
        b.tokens - a.tokens ||
        b.calls - a.calls,
    )
    .slice(0, Math.max(0, n));
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
  if (n.startsWith("minmax") || n.startsWith("minimax")) return "minimax";
  if (n.startsWith("mimo")) return "mimo";
  if (n.startsWith("llama")) return "meta";
  if (n.startsWith("ep-")) return "volcengine";
  return "other";
}

/** 将按模型排行聚合为按厂商排行。 */
export function aggregateByProvider(models: InsightsRankItem[]): InsightsRankItem[] {
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
