/** 模型市场：浏览 OpenRouter 全量模型目录，支持搜索、筛选、排序和详情面板。 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  ArrowDownAZ,
  ArrowUpAZ,
  Brain,
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

function ModelDetailPanel({ model }: { model: ModelCatalogEntry }) {
  const { t } = useI18n();
  const provider = providerFromId(model.id);
  const name = stripProviderPrefix(model.name ?? modelSlug(model.id), provider);
  const caps: { key: string; label: string; Icon: typeof Brain; has: boolean }[] = [
    { key: "tools", label: t("modelMarket.filter.tools" as never), Icon: Wrench, has: model.supports_function_calling },
    { key: "reasoning", label: t("modelMarket.filter.reasoning" as never), Icon: Brain, has: model.supports_reasoning },
    { key: "vision", label: t("modelMarket.filter.vision" as never), Icon: Eye, has: model.supports_vision },
    { key: "audio", label: t("modelMarket.filter.audio" as never), Icon: Headphones, has: model.supports_audio_input || model.supports_audio_output },
    { key: "image", label: t("modelMarket.filter.image" as never), Icon: Image, has: model.supports_image_generation },
    { key: "web", label: t("modelMarket.filter.all" as never), Icon: Globe, has: model.supports_web_search },
  ];
  const activeCaps = caps.filter((c) => c.has);

  return (
    <div className="mm-detail-panel">
      <div className="mm-detail-head">
        <span className="mm-detail-logo">
          <ModelBrandIcon modelId={model.id} width={32} height={32} />
        </span>
        <div className="mm-detail-titles">
          <span className="mm-detail-provider">{provider}</span>
          <span className="mm-detail-name">{name}</span>
        </div>
      </div>
      <div className="mm-detail-id-row">
        <code className="mm-detail-id-chip">{model.id}</code>
      </div>

      {model.description && (
        <div className="mm-detail-section">
          <span className="mm-detail-label">{t("modelMarket.detail.description" as never)}</span>
          <div className="mm-detail-body">{model.description}</div>
        </div>
      )}

      <div className="mm-detail-section">
        <span className="mm-detail-label">{t("modelMarket.detail.specs" as never)}</span>
        <div className="mm-detail-meta-grid">
          <div className="mm-detail-meta-item">
            <span className="mm-detail-meta-key">{t("modelMarket.context")}</span>
            <span className="mm-detail-meta-val">{formatCtx(model.context_length)}</span>
          </div>
          {model.pricing && (
            <>
              <div className="mm-detail-meta-item">
                <span className="mm-detail-meta-key">{t("modelMarket.detail.promptPrice" as never)}</span>
                <span className="mm-detail-meta-val price">{formatPrice(model.pricing.prompt_per_million)}/M</span>
              </div>
              <div className="mm-detail-meta-item">
                <span className="mm-detail-meta-key">{t("modelMarket.detail.completionPrice" as never)}</span>
                <span className="mm-detail-meta-val price">{formatPrice(model.pricing.completion_per_million)}/M</span>
              </div>
              {model.pricing.cache_read_per_million != null && model.pricing.cache_read_per_million > 0 && (
                <div className="mm-detail-meta-item">
                  <span className="mm-detail-meta-key">{t("modelMarket.detail.cacheRead" as never)}</span>
                  <span className="mm-detail-meta-val">{formatPrice(model.pricing.cache_read_per_million)}/M</span>
                </div>
              )}
              {model.pricing.cache_write_per_million != null && model.pricing.cache_write_per_million > 0 && (
                <div className="mm-detail-meta-item">
                  <span className="mm-detail-meta-key">{t("modelMarket.detail.cacheWrite" as never)}</span>
                  <span className="mm-detail-meta-val">{formatPrice(model.pricing.cache_write_per_million)}/M</span>
                </div>
              )}
            </>
          )}
          {model.knowledge_cutoff && (
            <div className="mm-detail-meta-item">
              <span className="mm-detail-meta-key">{t("modelMarket.detail.cutoff" as never)}</span>
              <span className="mm-detail-meta-val">{model.knowledge_cutoff}</span>
            </div>
          )}
          {model.created && (
            <div className="mm-detail-meta-item">
              <span className="mm-detail-meta-key">{t("modelMarket.detail.created" as never)}</span>
              <span className="mm-detail-meta-val">{new Date(model.created * 1000).toLocaleDateString()}</span>
            </div>
          )}
          {model.expiration_date && (
            <div className="mm-detail-meta-item">
              <span className="mm-detail-meta-key">{t("modelMarket.detail.expiration" as never)}</span>
              <span className="mm-detail-meta-val">{model.expiration_date}</span>
            </div>
          )}
        </div>
      </div>

      {activeCaps.length > 0 && (
        <div className="mm-detail-section">
          <span className="mm-detail-label">{t("modelMarket.detail.capabilities" as never)}</span>
          <div className="mm-detail-caps">
            {activeCaps.map((c) => (
              <span key={c.key} className="mm-detail-cap">
                <c.Icon size={13} />
                {c.label}
              </span>
            ))}
          </div>
        </div>
      )}

      {(model.input_modalities.length > 0 || model.output_modalities.length > 0) && (
        <div className="mm-detail-section">
          <span className="mm-detail-label">{t("modelMarket.detail.modalities" as never)}</span>
          <div className="mm-detail-meta-grid">
            {model.input_modalities.length > 0 && (
              <div className="mm-detail-meta-item">
                <span className="mm-detail-meta-key">Input</span>
                <span className="mm-detail-meta-val">{model.input_modalities.join(", ")}</span>
              </div>
            )}
            {model.output_modalities.length > 0 && (
              <div className="mm-detail-meta-item">
                <span className="mm-detail-meta-key">Output</span>
                <span className="mm-detail-meta-val">{model.output_modalities.join(", ")}</span>
              </div>
            )}
          </div>
        </div>
      )}
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
  const [selectedModelId, setSelectedModelId] = useState<string | null>(null);

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

  const PAGE_SIZE = 50;
  const [visibleCount, setVisibleCount] = useState(PAGE_SIZE);
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    setVisibleCount(PAGE_SIZE);
  }, [filter, search, sort, sortAsc]);

  useEffect(() => {
    const el = listRef.current;
    if (!el) return;
    const onScroll = () => {
      if (el.scrollTop + el.clientHeight >= el.scrollHeight - 200) {
        setVisibleCount((v) => Math.min(v + PAGE_SIZE, filtered.length));
      }
    };
    el.addEventListener("scroll", onScroll, { passive: true });
    return () => el.removeEventListener("scroll", onScroll);
  }, [filtered.length]);

  const visible = filtered.slice(0, visibleCount);

  useEffect(() => {
    if (viewMode !== "detail") return;
    if (selectedModelId && filtered.some((m) => m.id === selectedModelId)) return;
    setSelectedModelId(filtered[0]?.id ?? null);
  }, [viewMode, filtered, selectedModelId]);

  const selectedModel = viewMode === "detail"
    ? filtered.find((m) => m.id === selectedModelId) ?? null
    : null;

  if (!active) return null;

  const SORTS: { key: SortKey; labelKey: string }[] = [
    { key: "price", labelKey: "modelMarket.sort.price" },
    { key: "context", labelKey: "modelMarket.sort.context" },
    { key: "newest", labelKey: "modelMarket.sort.newest" },
    { key: "name", labelKey: "modelMarket.sort.name" },
  ];

  return (
    <div className="model-market">
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

        {/* List / Detail split */}
        {viewMode === "detail" ? (
          <div className="mm-detail-split">
            <div className="mm-detail-list" ref={listRef} role="list">
              {filtered.length === 0 && !loading && (
                <p className="model-market-empty">
                  {models.length === 0 ? t("modelMarket.empty") : t("modelMarket.noResults")}
                </p>
              )}
              {visible.map((m) => {
                const provider = providerFromId(m.id);
                const selected = m.id === selectedModelId;
                return (
                  <button
                    key={m.id}
                    type="button"
                    className={`mm-detail-item ${selected ? "is-selected" : ""}`}
                    role="listitem"
                    onClick={() => setSelectedModelId(m.id)}
                  >
                    <span className="mm-detail-item-logo">
                      <ModelBrandIcon modelId={m.id} width={18} height={18} />
                    </span>
                    <div className="mm-detail-item-titles">
                      <span className="mm-detail-item-provider">{provider}</span>
                      <span className="mm-detail-item-name">
                        {stripProviderPrefix(m.name ?? modelSlug(m.id), provider)}
                      </span>
                    </div>
                    <span className="mm-detail-item-ctx">{formatCtx(m.context_length)}</span>
                  </button>
                );
              })}
            </div>
            {selectedModel ? (
              <ModelDetailPanel key={selectedModel.id} model={selectedModel} />
            ) : (
              <div className="mm-detail-panel mm-detail-empty">
                <span>{t("modelMarket.detail.selectHint" as never)}</span>
              </div>
            )}
          </div>
        ) : (
          <div ref={listRef} className={`model-market-list view-${viewMode}`}>
            {filtered.length === 0 && !loading && (
              <p className="model-market-empty">
                {models.length === 0 ? t("modelMarket.empty") : t("modelMarket.noResults")}
              </p>
            )}
            {visible.map((m, idx) => (
              <div
                key={m.id}
                className="model-market-card mm-fade-in"
                style={{ animationDelay: `${Math.min(idx % PAGE_SIZE, 15) * 25}ms` }}
              >
                <div className="model-market-card-head">
                  <span className="model-market-card-logo">
                    <ModelBrandIcon modelId={m.id} width={viewMode === "list" ? 16 : 20} height={viewMode === "list" ? 16 : 20} />
                  </span>
                  <div className="model-market-card-titles">
                    <span className="model-market-card-provider">{providerFromId(m.id)}</span>
                    <span className="model-market-card-name">
                      {stripProviderPrefix(m.name ?? modelSlug(m.id), providerFromId(m.id))}
                    </span>
                  </div>
                  {m.created && (
                    <span className="model-market-card-age">{timeSince(m.created)}</span>
                  )}
                </div>
                <div className="model-market-card-meta">
                  <span className="model-market-chip" title={t("modelMarket.context")}>{formatCtx(m.context_length)}</span>
                  {m.pricing && (
                    <>
                      <span className="model-market-chip price">{formatPrice(m.pricing.prompt_per_million)}</span>
                      <span className="model-market-chip-sep">/</span>
                      <span className="model-market-chip price">{formatPrice(m.pricing.completion_per_million)}</span>
                    </>
                  )}
                  <CapabilityBadges m={m} />
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
