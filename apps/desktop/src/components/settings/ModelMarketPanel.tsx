/** 模型市场：浏览 OpenRouter 全量模型目录，支持搜索、筛选和排序。 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Brain,
  Eye,
  Headphones,
  Image,
  RefreshCw,
  Search,
  Wrench,
  Globe,
  Sparkles,
} from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";

interface ModelPricing {
  prompt_per_million: number | null;
  completion_per_million: number | null;
  cache_read_per_million: number | null;
  cache_write_per_million: number | null;
}

interface ModelCatalogEntry {
  id: string;
  name: string | null;
  description: string | null;
  context_length: number | null;
  created: number | null;
  pricing: ModelPricing | null;
  supports_vision: boolean;
  supports_function_calling: boolean;
  supports_reasoning: boolean;
  supports_web_search: boolean;
  supports_image_generation: boolean;
  supports_audio_input: boolean;
  supports_audio_output: boolean;
  input_modalities: string[];
  output_modalities: string[];
  knowledge_cutoff: string | null;
  expiration_date: string | null;
}

type SortKey = "price" | "context" | "newest" | "name";
type FilterKey =
  | "all"
  | "tools"
  | "reasoning"
  | "vision"
  | "audio"
  | "image"
  | "free";

function formatCtx(n: number | null): string {
  if (!n) return "—";
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(n % 1_000_000 === 0 ? 0 : 1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(0)}K`;
  return String(n);
}

function formatPrice(n: number | null | undefined): string {
  if (n == null) return "—";
  if (n === 0) return "Free";
  if (n < 0.01) return `$${n.toFixed(4)}`;
  if (n < 1) return `$${n.toFixed(2)}`;
  return `$${n.toFixed(2)}`;
}

function providerFromId(id: string): string {
  const slash = id.indexOf("/");
  return slash > 0 ? id.slice(0, slash) : id;
}

function timeSince(ts: number | null): string {
  if (!ts) return "";
  const now = Date.now() / 1000;
  const days = Math.floor((now - ts) / 86400);
  if (days < 1) return "today";
  if (days < 7) return `${days}d ago`;
  if (days < 30) return `${Math.floor(days / 7)}w ago`;
  if (days < 365) return `${Math.floor(days / 30)}mo ago`;
  return `${Math.floor(days / 365)}y ago`;
}

const FILTERS: { key: FilterKey; Icon: typeof Brain }[] = [
  { key: "all", Icon: Sparkles },
  { key: "tools", Icon: Wrench },
  { key: "reasoning", Icon: Brain },
  { key: "vision", Icon: Eye },
  { key: "audio", Icon: Headphones },
  { key: "image", Icon: Image },
  { key: "free", Icon: Globe },
];

export default function ModelMarketPanel({ active }: { active: boolean }) {
  const { t } = useI18n();
  const [models, setModels] = useState<ModelCatalogEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [search, setSearch] = useState("");
  const [sort, setSort] = useState<SortKey>("newest");
  const [filter, setFilter] = useState<FilterKey>("all");

  const load = useCallback(async (force: boolean) => {
    setLoading(true);
    try {
      const data = await invoke<ModelCatalogEntry[]>("list_model_catalog", {
        forceRefresh: force,
      });
      setModels(data);
    } catch (e) {
      console.error("Failed to load model catalog", e);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (active && models.length === 0) {
      void load(false);
    }
  }, [active, models.length, load]);

  const filtered = useMemo(() => {
    let list = models;

    if (filter !== "all") {
      list = list.filter((m) => {
        switch (filter) {
          case "tools": return m.supports_function_calling;
          case "reasoning": return m.supports_reasoning;
          case "vision": return m.supports_vision;
          case "audio": return m.supports_audio_input || m.supports_audio_output;
          case "image": return m.supports_image_generation;
          case "free":
            return (
              m.pricing?.prompt_per_million === 0 &&
              m.pricing?.completion_per_million === 0
            );
        }
      });
    }

    if (search.trim()) {
      const q = search.toLowerCase();
      list = list.filter(
        (m) =>
          m.id.toLowerCase().includes(q) ||
          (m.name ?? "").toLowerCase().includes(q) ||
          (m.description ?? "").toLowerCase().includes(q),
      );
    }

    list = [...list].sort((a, b) => {
      switch (sort) {
        case "price": {
          const pa = a.pricing?.prompt_per_million ?? Infinity;
          const pb = b.pricing?.prompt_per_million ?? Infinity;
          return pa - pb;
        }
        case "context":
          return (b.context_length ?? 0) - (a.context_length ?? 0);
        case "newest":
          return (b.created ?? 0) - (a.created ?? 0);
        case "name":
          return (a.name ?? a.id).localeCompare(b.name ?? b.id);
      }
    });

    return list;
  }, [models, filter, search, sort]);

  if (!active) return null;

  const SORTS: { key: SortKey; labelKey: string }[] = [
    { key: "price", labelKey: "modelMarket.sort.price" },
    { key: "context", labelKey: "modelMarket.sort.context" },
    { key: "newest", labelKey: "modelMarket.sort.newest" },
    { key: "name", labelKey: "modelMarket.sort.name" },
  ];

  return (
    <div className="model-market">
      {/* Toolbar */}
      <div className="model-market-toolbar">
        <div className="model-market-search">
          <Search size={14} />
          <input
            type="text"
            placeholder={t("modelMarket.search")}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>

        <div className="model-market-sort">
          {SORTS.map(({ key, labelKey }) => (
            <button
              key={key}
              type="button"
              className={`model-market-sort-btn ${sort === key ? "active" : ""}`}
              onClick={() => setSort(key)}
            >
              {t(labelKey as never)}
            </button>
          ))}
        </div>

        <button
          type="button"
          className="model-market-refresh"
          onClick={() => void load(true)}
          disabled={loading}
          title={t("modelMarket.refresh")}
        >
          <RefreshCw size={14} className={loading ? "spin" : ""} />
        </button>
      </div>

      {/* Filters */}
      <div className="model-market-filters">
        {FILTERS.map(({ key, Icon }) => (
          <button
            key={key}
            type="button"
            className={`model-market-filter-btn ${filter === key ? "active" : ""}`}
            onClick={() => setFilter(key)}
          >
            <Icon size={13} />
            {t(`modelMarket.filter.${key}` as never)}
          </button>
        ))}
        <span className="model-market-count">
          {t("modelMarket.total", { count: String(filtered.length) })}
        </span>
      </div>

      {/* List */}
      <div className="model-market-list">
        {filtered.length === 0 && !loading && (
          <p className="model-market-empty">
            {models.length === 0
              ? t("modelMarket.empty")
              : t("modelMarket.noResults")}
          </p>
        )}
        {filtered.map((m) => (
          <div key={m.id} className="model-market-card">
            <div className="model-market-card-head">
              <span className="model-market-card-provider">
                {providerFromId(m.id)}
              </span>
              <span className="model-market-card-name">
                {m.name ?? m.id}
              </span>
              {m.created && (
                <span className="model-market-card-age">
                  {timeSince(m.created)}
                </span>
              )}
            </div>

            <div className="model-market-card-meta">
              <span className="model-market-chip" title={t("modelMarket.context")}>
                {formatCtx(m.context_length)}
              </span>
              {m.pricing && (
                <>
                  <span
                    className="model-market-chip price"
                    title={`${t("modelMarket.inputPrice")}${t("modelMarket.perMillion")}`}
                  >
                    {formatPrice(m.pricing.prompt_per_million)}
                  </span>
                  <span className="model-market-chip-sep">/</span>
                  <span
                    className="model-market-chip price"
                    title={`${t("modelMarket.outputPrice")}${t("modelMarket.perMillion")}`}
                  >
                    {formatPrice(m.pricing.completion_per_million)}
                  </span>
                </>
              )}
            </div>

            <div className="model-market-card-caps">
              {m.supports_function_calling && (
                <span className="model-market-cap" title="Tools">
                  <Wrench size={11} />
                </span>
              )}
              {m.supports_reasoning && (
                <span className="model-market-cap" title="Reasoning">
                  <Brain size={11} />
                </span>
              )}
              {m.supports_vision && (
                <span className="model-market-cap" title="Vision">
                  <Eye size={11} />
                </span>
              )}
              {m.supports_audio_input && (
                <span className="model-market-cap" title="Audio">
                  <Headphones size={11} />
                </span>
              )}
              {m.supports_image_generation && (
                <span className="model-market-cap" title="Image Gen">
                  <Image size={11} />
                </span>
              )}
              {m.supports_web_search && (
                <span className="model-market-cap" title="Web Search">
                  <Globe size={11} />
                </span>
              )}
            </div>

            {m.description && (
              <p className="model-market-card-desc">
                {m.description.length > 120
                  ? m.description.slice(0, 120) + "…"
                  : m.description}
              </p>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
