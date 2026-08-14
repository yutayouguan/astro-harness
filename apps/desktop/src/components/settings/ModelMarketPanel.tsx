/** 模型市场：浏览 OpenRouter 全量模型目录，支持搜索、筛选、排序和详情面板。 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  ArrowDownAZ,
  ArrowUpAZ,
  Brain,
  ChevronRight,
  Eye,
  Grid2x2,
  Headphones,
  Image,
  Columns2,
  List,
  RefreshCw,
  Search,
  Wrench,
  Globe,
  Sparkles,
  X,
} from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";
import { ModelBrandIcon } from "../icons/ProviderIcons";

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

function modelSlug(id: string): string {
  const slash = id.indexOf("/");
  return slash > 0 ? id.slice(slash + 1) : id;
}

function stripProviderPrefix(name: string, provider: string): string {
  const prefixes = [
    `${provider}: `,
    `${provider.charAt(0).toUpperCase() + provider.slice(1)}: `,
    `${provider.toUpperCase()}: `,
  ];
  for (const p of prefixes) {
    if (name.startsWith(p)) return name.slice(p.length);
  }
  const colonIdx = name.indexOf(": ");
  if (colonIdx > 0 && colonIdx < 20) {
    const before = name.slice(0, colonIdx).toLowerCase();
    if (provider.toLowerCase().includes(before) || before.includes(provider.toLowerCase())) {
      return name.slice(colonIdx + 2);
    }
  }
  return name;
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

function formatDate(ts: number | null): string {
  if (!ts) return "—";
  return new Date(ts * 1000).toLocaleDateString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
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

function CapabilityBadges({ m }: { m: ModelCatalogEntry }) {
  return (
    <div className="model-market-card-caps">
      {m.supports_function_calling && (
        <span className="model-market-cap" title="Tools"><Wrench size={11} /></span>
      )}
      {m.supports_reasoning && (
        <span className="model-market-cap" title="Reasoning"><Brain size={11} /></span>
      )}
      {m.supports_vision && (
        <span className="model-market-cap" title="Vision"><Eye size={11} /></span>
      )}
      {m.supports_audio_input && (
        <span className="model-market-cap" title="Audio Input"><Headphones size={11} /></span>
      )}
      {m.supports_image_generation && (
        <span className="model-market-cap" title="Image Gen"><Image size={11} /></span>
      )}
      {m.supports_web_search && (
        <span className="model-market-cap" title="Web Search"><Globe size={11} /></span>
      )}
    </div>
  );
}

function ModelDetail({ m, onClose }: { m: ModelCatalogEntry; onClose: () => void }) {
  const { t } = useI18n();
  return (
    <div className="model-market-detail">
      <div className="model-market-detail-header">
        <div className="model-market-detail-brand">
          <ModelBrandIcon modelId={m.id} width={28} height={28} />
        </div>
        <div className="model-market-detail-title">
          <span className="model-market-detail-provider">{providerFromId(m.id)}</span>
          <h3 className="model-market-detail-name">{stripProviderPrefix(m.name ?? modelSlug(m.id), providerFromId(m.id))}</h3>
        </div>
        <button type="button" className="model-market-detail-close" onClick={onClose}>
          <X size={16} />
        </button>
      </div>

      <div className="model-market-detail-body">
        <div className="model-market-detail-section">
          <h4>{t("modelMarket.context")}</h4>
          <span className="model-market-detail-value">{formatCtx(m.context_length)} tokens</span>
        </div>

        <div className="model-market-detail-section">
          <h4>{t("modelMarket.sort.price")} ($/1M tokens)</h4>
          <div className="model-market-detail-prices">
            <div className="model-market-detail-price-row">
              <span>{t("modelMarket.inputPrice")}</span>
              <span className="model-market-detail-value">
                {formatPrice(m.pricing?.prompt_per_million)}
              </span>
            </div>
            <div className="model-market-detail-price-row">
              <span>{t("modelMarket.outputPrice")}</span>
              <span className="model-market-detail-value">
                {formatPrice(m.pricing?.completion_per_million)}
              </span>
            </div>
            {m.pricing?.cache_read_per_million != null && (
              <div className="model-market-detail-price-row">
                <span>Cache Read</span>
                <span className="model-market-detail-value">
                  {formatPrice(m.pricing.cache_read_per_million)}
                </span>
              </div>
            )}
          </div>
        </div>

        <div className="model-market-detail-section">
          <h4>Capabilities</h4>
          <CapabilityBadges m={m} />
        </div>

        {(m.input_modalities.length > 0 || m.output_modalities.length > 0) && (
          <div className="model-market-detail-section">
            <h4>Modalities</h4>
            {m.input_modalities.length > 0 && (
              <div className="model-market-detail-modalities">
                <span className="model-market-detail-mod-label">Input:</span>
                {m.input_modalities.map((mod) => (
                  <span key={mod} className="model-market-chip">{mod}</span>
                ))}
              </div>
            )}
            {m.output_modalities.length > 0 && (
              <div className="model-market-detail-modalities">
                <span className="model-market-detail-mod-label">Output:</span>
                {m.output_modalities.map((mod) => (
                  <span key={mod} className="model-market-chip">{mod}</span>
                ))}
              </div>
            )}
          </div>
        )}

        {m.knowledge_cutoff && (
          <div className="model-market-detail-section">
            <h4>Knowledge Cutoff</h4>
            <span className="model-market-detail-value">{m.knowledge_cutoff}</span>
          </div>
        )}

        {m.created && (
          <div className="model-market-detail-section">
            <h4>Added to OpenRouter</h4>
            <span className="model-market-detail-value">{formatDate(m.created)}</span>
          </div>
        )}

        {m.expiration_date && (
          <div className="model-market-detail-section">
            <h4>Expiration</h4>
            <span className="model-market-detail-value warn">{m.expiration_date}</span>
          </div>
        )}

        {m.description && (
          <div className="model-market-detail-section">
            <h4>Description</h4>
            <p className="model-market-detail-desc">{m.description}</p>
          </div>
        )}

        <div className="model-market-detail-section">
          <h4>Model ID</h4>
          <code className="model-market-detail-id">{m.id}</code>
        </div>
      </div>
    </div>
  );
}

export default function ModelMarketPanel({ active }: { active: boolean }) {
  const { t } = useI18n();
  const [models, setModels] = useState<ModelCatalogEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [search, setSearch] = useState("");
  const [sort, setSort] = useState<SortKey>("newest");
  const [sortAsc, setSortAsc] = useState(false);
  const [filter, setFilter] = useState<FilterKey>("all");
  const [viewMode, setViewMode] = useState<"gallery" | "list" | "detail">("gallery");
  const [selectedId, setSelectedId] = useState<string | null>(null);

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
      let cmp: number;
      switch (sort) {
        case "price": {
          const pa = a.pricing?.prompt_per_million ?? Infinity;
          const pb = b.pricing?.prompt_per_million ?? Infinity;
          cmp = pa - pb;
          break;
        }
        case "context":
          cmp = (b.context_length ?? 0) - (a.context_length ?? 0);
          break;
        case "newest":
          cmp = (b.created ?? 0) - (a.created ?? 0);
          break;
        case "name":
          cmp = (a.name ?? a.id).localeCompare(b.name ?? b.id);
          break;
      }
      return sortAsc ? -cmp : cmp;
    });

    return list;
  }, [models, filter, search, sort, sortAsc]);

  const selected = selectedId ? models.find((m) => m.id === selectedId) ?? null : null;

  if (!active) return null;

  const SORTS: { key: SortKey; labelKey: string }[] = [
    { key: "price", labelKey: "modelMarket.sort.price" },
    { key: "context", labelKey: "modelMarket.sort.context" },
    { key: "newest", labelKey: "modelMarket.sort.newest" },
    { key: "name", labelKey: "modelMarket.sort.name" },
  ];

  return (
    <div className={`model-market ${selected ? "has-detail" : ""}`}>
      <div className="model-market-main">
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
            {search && (
              <button
                type="button"
                className="model-market-search-clear"
                onClick={() => setSearch("")}
              >
                <X size={13} />
              </button>
            )}
          </div>

          <div className="model-market-sort">
            {SORTS.map(({ key, labelKey }) => (
              <button
                key={key}
                type="button"
                className={`model-market-sort-btn ${sort === key ? "active" : ""}`}
                onClick={() => {
                  if (sort === key) {
                    setSortAsc((v) => !v);
                  } else {
                    setSort(key);
                    setSortAsc(false);
                  }
                }}
              >
                {t(labelKey as never)}
                {sort === key && (
                  sortAsc
                    ? <ArrowUpAZ size={13} className="model-market-sort-arrow" />
                    : <ArrowDownAZ size={13} className="model-market-sort-arrow" />
                )}
              </button>
            ))}
          </div>

          <div className="model-market-view-toggle">
            <button
              type="button"
              className={`model-market-view-btn ${viewMode === "gallery" ? "active" : ""}`}
              onClick={() => setViewMode("gallery")}
              title={t("modelMarket.view.gallery" as never)}
            >
              <Grid2x2 size={14} />
            </button>
            <button
              type="button"
              className={`model-market-view-btn ${viewMode === "list" ? "active" : ""}`}
              onClick={() => setViewMode("list")}
              title={t("modelMarket.view.list" as never)}
            >
              <List size={14} />
            </button>
            <button
              type="button"
              className={`model-market-view-btn ${viewMode === "detail" ? "active" : ""}`}
              onClick={() => setViewMode("detail")}
              title={t("modelMarket.view.detail" as never)}
            >
              <Columns2 size={14} />
            </button>
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
        <div className={`model-market-list view-${viewMode}`}>
          {filtered.length === 0 && !loading && (
            <p className="model-market-empty">
              {models.length === 0
                ? t("modelMarket.empty")
                : t("modelMarket.noResults")}
            </p>
          )}
          {filtered.map((m) => (
            <button
              key={m.id}
              type="button"
              className={`model-market-card ${selectedId === m.id ? "is-selected" : ""}`}
              onClick={() => setSelectedId(m.id === selectedId ? null : m.id)}
            >
              <div className="model-market-card-head">
                <span className="model-market-card-logo">
                  <ModelBrandIcon modelId={m.id} width={viewMode === "list" ? 16 : 20} height={viewMode === "list" ? 16 : 20} />
                </span>
                <div className="model-market-card-titles">
                  <span className="model-market-card-provider">
                    {providerFromId(m.id)}
                  </span>
                  <span className="model-market-card-name">
                    {stripProviderPrefix(m.name ?? modelSlug(m.id), providerFromId(m.id))}
                  </span>
                </div>
                {m.created && (
                  <span className="model-market-card-age">
                    {timeSince(m.created)}
                  </span>
                )}
                <ChevronRight size={14} className="model-market-card-arrow" />
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
                <CapabilityBadges m={m} />
              </div>

              {viewMode === "detail" && m.description && (
                <p className="model-market-card-desc">
                  {m.description.length > 200
                    ? m.description.slice(0, 200) + "…"
                    : m.description}
                </p>
              )}
            </button>
          ))}
        </div>
      </div>

      {/* Detail panel */}
      {selected && (
        <ModelDetail m={selected} onClose={() => setSelectedId(null)} />
      )}
    </div>
  );
}
