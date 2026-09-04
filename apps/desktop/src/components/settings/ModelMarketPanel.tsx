/** 模型市场：浏览 OpenRouter 全量模型目录，支持搜索、筛选、排序和详情面板。 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  ArrowDownAZ,
  ArrowUpAZ,
  Binary,
  BookOpen,
  Brain,
  Calendar,
  Coins,
  Eye,
  FileText,
  Grid2x2,
  Headphones,
  Image,
  Columns2,
  Layers,
  List,
  ListOrdered,
  BarChart3,
  RefreshCw,
  Search,
  Shield,
  Wrench,
  Globe,
  Sparkles,
  X,
  Zap,
} from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";
import {
  matchesModelMarketFilter,
  matchesModelMarketType,
  providerSaveInputWithEmbedding,
  type ModelCatalogEntry,
  type ModelMarketFilter,
  type ModelMarketTypeFilter,
} from "../../lib/model/modelMarket";
import type { ProviderDto, ProvidersStateDto } from "../../types";
import { ModelBrandIcon } from "../icons/ProviderIcons";
import ModelRankingsPanel from "./ModelRankingsPanel";

type SortKey = "price" | "context" | "newest" | "name";

function formatCtx(n: number | null): string {
  if (!n) return "—";
  if (n >= 1_000_000)
    return `${(n / 1_000_000).toFixed(n % 1_000_000 === 0 ? 0 : 1)}M`;
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
    if (
      provider.toLowerCase().includes(before) ||
      before.includes(provider.toLowerCase())
    ) {
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

const MODEL_TYPES: { key: ModelMarketTypeFilter; Icon: typeof Brain }[] = [
  { key: "all", Icon: Sparkles },
  { key: "generation", Icon: Brain },
  { key: "embedding", Icon: Binary },
  { key: "rerank", Icon: ListOrdered },
];

const FILTERS: { key: ModelMarketFilter; Icon: typeof Brain }[] = [
  { key: "all", Icon: Sparkles },
  { key: "tools", Icon: Wrench },
  { key: "reasoning", Icon: Brain },
  { key: "vision", Icon: Eye },
  { key: "audio", Icon: Headphones },
  { key: "image", Icon: Image },
  { key: "free", Icon: Globe },
];

type ModelMarketPanelProps = {
  active: boolean;
  onProvidersStateChange?: (state: ProvidersStateDto) => void;
};

type EmbeddingSaveState = {
  modelId: string;
  status: "saving" | "saved" | "error";
  message?: string;
};

function CapabilityBadges({ m }: { m: ModelCatalogEntry }) {
  return (
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
        <span className="model-market-cap" title="Audio Input">
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
      {m.model_type === "embedding" && (
        <span className="model-market-cap" title="Embedding">
          <Binary size={11} />
        </span>
      )}
      {m.model_type === "rerank" && (
        <span className="model-market-cap" title="Rerank">
          <ListOrdered size={11} />
        </span>
      )}
    </div>
  );
}

function ModelDetailPanel({
  model,
  openRouterProvider,
  embeddingSave,
  onConfigureEmbedding,
}: {
  model: ModelCatalogEntry;
  openRouterProvider: ProviderDto | null;
  embeddingSave: EmbeddingSaveState | null;
  onConfigureEmbedding: (model: ModelCatalogEntry) => void;
}) {
  const { t } = useI18n();
  const provider = providerFromId(model.id);
  const name = stripProviderPrefix(model.name ?? modelSlug(model.id), provider);
  const caps: {
    key: string;
    label: string;
    Icon: typeof Brain;
    has: boolean;
  }[] = [
    {
      key: "tools",
      label: t("modelMarket.filter.tools" as never),
      Icon: Wrench,
      has: model.supports_function_calling,
    },
    {
      key: "reasoning",
      label: t("modelMarket.filter.reasoning" as never),
      Icon: Brain,
      has: model.supports_reasoning,
    },
    {
      key: "vision",
      label: t("modelMarket.filter.vision" as never),
      Icon: Eye,
      has: model.supports_vision,
    },
    {
      key: "audio",
      label: t("modelMarket.filter.audio" as never),
      Icon: Headphones,
      has: model.supports_audio_input || model.supports_audio_output,
    },
    {
      key: "image",
      label: t("modelMarket.filter.image" as never),
      Icon: Image,
      has: model.supports_image_generation,
    },
    {
      key: "embedding",
      label: t("modelMarket.filter.embedding" as never),
      Icon: Binary,
      has: model.model_type === "embedding",
    },
    {
      key: "rerank",
      label: t("modelMarket.filter.rerank" as never),
      Icon: ListOrdered,
      has: model.model_type === "rerank",
    },
    {
      key: "web",
      label: t("modelMarket.capability.web" as never),
      Icon: Globe,
      has: model.supports_web_search,
    },
  ];
  const activeCaps = caps.filter((c) => c.has);
  const isEmbedding = model.model_type === "embedding";
  const isRerank = model.model_type === "rerank";
  const embeddingConfigured =
    isEmbedding && openRouterProvider?.embedding_model === model.id;
  const embeddingReady = Boolean(
    embeddingConfigured &&
    openRouterProvider?.enabled &&
    openRouterProvider.has_api_key &&
    openRouterProvider.supports_embedding,
  );
  const savingThisModel =
    embeddingSave?.modelId === model.id && embeddingSave.status === "saving";

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
          <span className="mm-detail-label">
            <FileText size={13} />{" "}
            {t("modelMarket.detail.description" as never)}
          </span>
          <div className="mm-detail-body">{model.description}</div>
        </div>
      )}

      <div className="mm-detail-section">
        <span className="mm-detail-label">
          <Coins size={13} /> {t("modelMarket.detail.specs" as never)}
        </span>
        <div className="mm-detail-meta-grid">
          <div className="mm-detail-meta-item">
            <span className="mm-detail-meta-key">
              <Layers size={12} /> {t("modelMarket.detail.type" as never)}
            </span>
            <span className="mm-detail-meta-val">
              {t(`modelMarket.type.${model.model_type}` as never)}
            </span>
          </div>
          <div className="mm-detail-meta-item">
            <span className="mm-detail-meta-key">
              <BookOpen size={12} />{" "}
              {t(
                model.model_type === "generation"
                  ? "modelMarket.context"
                  : ("modelMarket.detail.inputLimit" as never),
              )}
            </span>
            <span className="mm-detail-meta-val">
              {formatCtx(model.context_length)}
            </span>
          </div>
          {model.pricing && (
            <>
              <div className="mm-detail-meta-item">
                <span className="mm-detail-meta-key">
                  <Coins size={12} />{" "}
                  {t("modelMarket.detail.promptPrice" as never)}
                </span>
                <span className="mm-detail-meta-val price">
                  {formatPrice(model.pricing.prompt_per_million)}/M
                </span>
              </div>
              {model.model_type === "generation" && (
                <div className="mm-detail-meta-item">
                  <span className="mm-detail-meta-key">
                    <Coins size={12} />{" "}
                    {t("modelMarket.detail.completionPrice" as never)}
                  </span>
                  <span className="mm-detail-meta-val price">
                    {formatPrice(model.pricing.completion_per_million)}/M
                  </span>
                </div>
              )}
              {model.pricing.cache_read_per_million != null &&
                model.pricing.cache_read_per_million > 0 && (
                  <div className="mm-detail-meta-item">
                    <span className="mm-detail-meta-key">
                      <Coins size={12} />{" "}
                      {t("modelMarket.detail.cacheRead" as never)}
                    </span>
                    <span className="mm-detail-meta-val">
                      {formatPrice(model.pricing.cache_read_per_million)}/M
                    </span>
                  </div>
                )}
              {model.pricing.cache_write_per_million != null &&
                model.pricing.cache_write_per_million > 0 && (
                  <div className="mm-detail-meta-item">
                    <span className="mm-detail-meta-key">
                      <Coins size={12} />{" "}
                      {t("modelMarket.detail.cacheWrite" as never)}
                    </span>
                    <span className="mm-detail-meta-val">
                      {formatPrice(model.pricing.cache_write_per_million)}/M
                    </span>
                  </div>
                )}
            </>
          )}
          {model.knowledge_cutoff && (
            <div className="mm-detail-meta-item">
              <span className="mm-detail-meta-key">
                <Calendar size={12} /> {t("modelMarket.detail.cutoff" as never)}
              </span>
              <span className="mm-detail-meta-val">
                {model.knowledge_cutoff}
              </span>
            </div>
          )}
          {model.created && (
            <div className="mm-detail-meta-item">
              <span className="mm-detail-meta-key">
                <Calendar size={12} />{" "}
                {t("modelMarket.detail.created" as never)}
              </span>
              <span className="mm-detail-meta-val">
                {new Date(model.created * 1000).toLocaleDateString()}
              </span>
            </div>
          )}
          {model.expiration_date && (
            <div className="mm-detail-meta-item">
              <span className="mm-detail-meta-key">
                <Shield size={12} />{" "}
                {t("modelMarket.detail.expiration" as never)}
              </span>
              <span className="mm-detail-meta-val">
                {model.expiration_date}
              </span>
            </div>
          )}
        </div>
      </div>

      {activeCaps.length > 0 && (
        <div className="mm-detail-section">
          <span className="mm-detail-label">
            <Zap size={13} /> {t("modelMarket.detail.capabilities" as never)}
          </span>
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

      {(isEmbedding || isRerank) && (
        <div className="mm-detail-section">
          <span className="mm-detail-label">
            <Zap size={13} /> {t("modelMarket.runtime.title" as never)}
          </span>
          {isRerank ? (
            <p className="mm-runtime-copy is-unavailable">
              {t("modelMarket.runtime.rerankUnavailable" as never)}
            </p>
          ) : (
            <div className="mm-runtime-action">
              <div>
                <p
                  className={`mm-runtime-copy ${embeddingReady ? "is-ready" : ""}`}
                >
                  {embeddingReady
                    ? t("modelMarket.runtime.embeddingReady" as never)
                    : embeddingConfigured
                      ? t("modelMarket.runtime.embeddingNeedsProvider" as never)
                      : t("modelMarket.runtime.embeddingAvailable" as never)}
                </p>
                {embeddingSave?.modelId === model.id &&
                  embeddingSave.status === "error" && (
                    <p className="mm-runtime-error">{embeddingSave.message}</p>
                  )}
              </div>
              <button
                type="button"
                className="mm-runtime-button"
                disabled={
                  !openRouterProvider?.supports_embedding ||
                  savingThisModel ||
                  embeddingConfigured
                }
                onClick={() => onConfigureEmbedding(model)}
                title={
                  openRouterProvider
                    ? undefined
                    : t("modelMarket.runtime.openRouterMissing" as never)
                }
              >
                {savingThisModel
                  ? t("modelMarket.runtime.saving" as never)
                  : embeddingConfigured
                    ? t("modelMarket.runtime.configured" as never)
                    : t("modelMarket.runtime.configure" as never)}
              </button>
            </div>
          )}
        </div>
      )}

      {(model.input_modalities.length > 0 ||
        model.output_modalities.length > 0) && (
        <div className="mm-detail-section">
          <span className="mm-detail-label">
            <Layers size={13} /> {t("modelMarket.detail.modalities" as never)}
          </span>
          <div className="mm-detail-meta-grid">
            {model.input_modalities.length > 0 && (
              <div className="mm-detail-meta-item">
                <span className="mm-detail-meta-key">Input</span>
                <span className="mm-detail-meta-val">
                  {model.input_modalities.join(", ")}
                </span>
              </div>
            )}
            {model.output_modalities.length > 0 && (
              <div className="mm-detail-meta-item">
                <span className="mm-detail-meta-key">Output</span>
                <span className="mm-detail-meta-val">
                  {model.output_modalities.join(", ")}
                </span>
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

export default function ModelMarketPanel({
  active,
  onProvidersStateChange,
}: ModelMarketPanelProps) {
  const { t } = useI18n();
  const [surface, setSurface] = useState<"catalog" | "rankings">("catalog");
  const [models, setModels] = useState<ModelCatalogEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [search, setSearch] = useState("");
  const [sort, setSort] = useState<SortKey>("newest");
  const [sortAsc, setSortAsc] = useState(false);
  const [filter, setFilter] = useState<ModelMarketFilter>("all");
  const [modelType, setModelType] = useState<ModelMarketTypeFilter>("all");
  const [viewMode, setViewMode] = useState<"gallery" | "list" | "detail">(
    "gallery",
  );
  const [selectedModelId, setSelectedModelId] = useState<string | null>(null);
  const [providersState, setProvidersState] =
    useState<ProvidersStateDto | null>(null);
  const [embeddingSave, setEmbeddingSave] = useState<EmbeddingSaveState | null>(
    null,
  );

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
    if (active && surface === "catalog" && models.length === 0) {
      void load(false);
    }
  }, [active, models.length, load, surface]);

  useEffect(() => {
    if (!active || surface !== "catalog") return;
    void invoke<ProvidersStateDto>("get_providers_state")
      .then(setProvidersState)
      .catch(() => setProvidersState(null));
  }, [active, surface]);

  const openRouterProvider = useMemo(
    () =>
      providersState?.providers.find(
        (provider) =>
          provider.kind === "openrouter" && provider.config_source !== "toml",
      ) ?? null,
    [providersState],
  );

  const configureEmbedding = useCallback(
    async (model: ModelCatalogEntry) => {
      if (!openRouterProvider || model.model_type !== "embedding") return;
      setEmbeddingSave({ modelId: model.id, status: "saving" });
      try {
        const next = await invoke<ProvidersStateDto>("save_provider", {
          provider: providerSaveInputWithEmbedding(
            openRouterProvider,
            model.id,
          ),
        });
        setProvidersState(next);
        onProvidersStateChange?.(next);
        setEmbeddingSave({ modelId: model.id, status: "saved" });
      } catch (error) {
        setEmbeddingSave({
          modelId: model.id,
          status: "error",
          message: String(error),
        });
      }
    },
    [onProvidersStateChange, openRouterProvider],
  );

  const filtered = useMemo(() => {
    let list = models;

    if (modelType !== "all") {
      list = list.filter((model) => matchesModelMarketType(model, modelType));
    }

    if (filter !== "all") {
      list = list.filter((model) => matchesModelMarketFilter(model, filter));
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
  }, [models, modelType, filter, search, sort, sortAsc]);

  const PAGE_SIZE = 50;
  const [visibleCount, setVisibleCount] = useState(PAGE_SIZE);
  const listRef = useRef<HTMLDivElement>(null);
  const observerRef = useRef<IntersectionObserver | null>(null);

  useEffect(() => {
    observerRef.current = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (entry.isIntersecting) {
            entry.target.classList.add("mm-visible");
          } else {
            entry.target.classList.remove("mm-visible");
          }
        }
      },
      { rootMargin: "80px 0px", threshold: 0.01 },
    );
    return () => observerRef.current?.disconnect();
  }, []);

  const cardRef = useCallback((el: HTMLDivElement | null) => {
    if (el) observerRef.current?.observe(el);
  }, []);

  useEffect(() => {
    setVisibleCount(PAGE_SIZE);
  }, [modelType, filter, search, sort, sortAsc]);

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
    if (selectedModelId && filtered.some((m) => m.id === selectedModelId))
      return;
    setSelectedModelId(filtered[0]?.id ?? null);
  }, [viewMode, filtered, selectedModelId]);

  const selectedModel =
    viewMode === "detail"
      ? (filtered.find((m) => m.id === selectedModelId) ?? null)
      : null;

  if (!active) return null;

  const SORTS: { key: SortKey; labelKey: string }[] = [
    { key: "price", labelKey: "modelMarket.sort.price" },
    { key: "context", labelKey: "modelMarket.sort.context" },
    { key: "newest", labelKey: "modelMarket.sort.newest" },
    { key: "name", labelKey: "modelMarket.sort.name" },
  ];

  const surfaceTabs = (
    <div className="model-market-surface-tabs" role="tablist">
      <button
        type="button"
        role="tab"
        aria-selected={surface === "catalog"}
        className={surface === "catalog" ? "active" : ""}
        onClick={() => setSurface("catalog")}
      >
        <Grid2x2 size={15} />
        {t("modelMarket.surface.catalog" as never)}
      </button>
      <button
        type="button"
        role="tab"
        aria-selected={surface === "rankings"}
        className={surface === "rankings" ? "active" : ""}
        onClick={() => setSurface("rankings")}
      >
        <BarChart3 size={15} />
        {t("modelMarket.surface.rankings" as never)}
      </button>
    </div>
  );

  if (surface === "rankings") {
    return (
      <div className="model-market">
        {surfaceTabs}
        <ModelRankingsPanel active={active} />
      </div>
    );
  }

  return (
    <div className="model-market">
      {surfaceTabs}
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
                {sort === key &&
                  (sortAsc ? (
                    <ArrowUpAZ size={13} className="model-market-sort-arrow" />
                  ) : (
                    <ArrowDownAZ
                      size={13}
                      className="model-market-sort-arrow"
                    />
                  ))}
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

        {/* Model types */}
        <div
          className="model-market-type-tabs"
          aria-label={t("modelMarket.type.label" as never)}
        >
          {MODEL_TYPES.map(({ key, Icon }) => {
            const count =
              key === "all"
                ? models.length
                : models.filter((model) => model.model_type === key).length;
            return (
              <button
                key={key}
                type="button"
                className={`model-market-type-btn ${modelType === key ? "active" : ""}`}
                onClick={() => setModelType(key)}
              >
                <Icon size={14} />
                {t(`modelMarket.type.${key}` as never)}
                <span>{count}</span>
              </button>
            );
          })}
        </div>

        {/* Capability filters */}
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
                  {models.length === 0
                    ? t("modelMarket.empty")
                    : t("modelMarket.noResults")}
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
                      <span className="mm-detail-item-provider">
                        {provider}
                      </span>
                      <span className="mm-detail-item-name">
                        {stripProviderPrefix(
                          m.name ?? modelSlug(m.id),
                          provider,
                        )}
                      </span>
                    </div>
                    <span className="mm-detail-item-ctx">
                      {formatCtx(m.context_length)}
                    </span>
                  </button>
                );
              })}
            </div>
            {selectedModel ? (
              <ModelDetailPanel
                key={selectedModel.id}
                model={selectedModel}
                openRouterProvider={openRouterProvider}
                embeddingSave={embeddingSave}
                onConfigureEmbedding={configureEmbedding}
              />
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
                {models.length === 0
                  ? t("modelMarket.empty")
                  : t("modelMarket.noResults")}
              </p>
            )}
            {visible.map((m) => (
              <div
                key={m.id}
                ref={cardRef}
                className="model-market-card mm-scroll-reveal"
              >
                <div className="model-market-card-head">
                  <span className="model-market-card-logo">
                    <ModelBrandIcon
                      modelId={m.id}
                      width={viewMode === "list" ? 16 : 20}
                      height={viewMode === "list" ? 16 : 20}
                    />
                  </span>
                  <div className="model-market-card-titles">
                    <span className="model-market-card-provider">
                      {providerFromId(m.id)}
                    </span>
                    <span className="model-market-card-name">
                      {stripProviderPrefix(
                        m.name ?? modelSlug(m.id),
                        providerFromId(m.id),
                      )}
                    </span>
                  </div>
                  {m.created && (
                    <span className="model-market-card-age">
                      {timeSince(m.created)}
                    </span>
                  )}
                </div>
                <div className="model-market-card-meta">
                  <span
                    className="model-market-chip"
                    title={t("modelMarket.context")}
                  >
                    {formatCtx(m.context_length)}
                  </span>
                  {m.pricing && (
                    <>
                      <span className="model-market-chip price">
                        {formatPrice(m.pricing.prompt_per_million)}
                      </span>
                      {m.model_type === "generation" && (
                        <>
                          <span className="model-market-chip-sep">/</span>
                          <span className="model-market-chip price">
                            {formatPrice(m.pricing.completion_per_million)}
                          </span>
                        </>
                      )}
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
