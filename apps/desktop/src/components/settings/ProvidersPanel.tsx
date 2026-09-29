/** 模型供应商配置、探测与模型列表。 */
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
  type SVGProps,
} from "react";
import {
  ArrowUpDown,
  BookOpen,
  Box,
  Brain,
  Calendar,
  CircleDollarSign,
  ExternalLink,
  Eye,
  FileText,
  Globe,
  GripVertical,
  Image,
  Info,
  KeyRound,
  Layers,
  Lightbulb,
  Link2,
  LoaderCircle,
  MessageCircle,
  Mic,
  MoreHorizontal,
  Music,
  Plus,
  RefreshCw,
  Save,
  Search,
  ShieldCheck,
  Sparkles,
  Star,
  Stethoscope,
  Tag,
  Timer,
  Trash2,
  Video,
  Waypoints,
  Wrench,
  Zap,
} from "lucide-react";
import { Eye as EyeData, EyeOff as EyeOffData } from "lucide";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { open as shellOpen } from "@tauri-apps/plugin-shell";
import { MorphToggleIcon } from "../icons/MorphIcon";
import { ModelBrandIcon, ProviderBrandIcon } from "../icons/ProviderIcons";
import { Button, IconButton, PopoverSurface, SelectMenu, Surface } from "../ui";
import { useConfirm } from "../../hooks/ui/DialogContext";
import { useI18n } from "../../i18n/LocaleContext";
import type { MessageKey } from "../../i18n/messages";
import { EmptyIllustration } from "../../illustrations";
import AuxiliaryModelsPanel from "./AuxiliaryModelsPanel";
import {
  compareModelsByCreatedDesc,
  formatContextWindow,
  formatKnowledgeCutoff,
  formatModelCreated,
  formatModelPrice,
  isModelCreatedWithinDays,
  listActiveModelCaps,
} from "../../lib/model/modelCaps";
import {
  buildMediaModelOptions,
  evaluateMediaModelsResult,
  isMediaModelsRequestLoading,
  sanitizeMediaModelValue,
  type MediaCapabilityKey,
} from "../../lib/providers/mediaModelOptions";
import type {
  ModelInfo,
  ProviderDto,
  ProviderFallbackEntry,
  ProviderKindId,
  ProviderModelsResult,
  ProviderTestResult,
  ProvidersStateDto,
} from "../../types";

/** 聊天后备链上限（与后端 / expand 一致） */
const MAX_MODEL_FALLBACKS = 3;

/** 规范化草稿中的后备列表（截断 + 空 model → null） */
function normalizeFallback(
  entries: ProviderFallbackEntry[] | undefined | null,
): ProviderFallbackEntry[] {
  return (entries ?? []).slice(0, MAX_MODEL_FALLBACKS).map((e) => {
    const model = e.model?.trim();
    return {
      provider_id: e.provider_id,
      model: model ? model : null,
    };
  });
}

function trimSlash(s: string): string {
  return s.replace(/\/+$/, "");
}

function resolveEndpointPreview(
  kind: string,
  endpoint: string,
  apiMode?: string,
): string {
  const base = trimSlash(endpoint.trim());
  if (!base) return "";
  if (apiMode === "responses") {
    const norm = /\/v1$/.test(base) ? base : `${base}/v1`;
    return `${norm}/responses`;
  }
  switch (kind) {
    case "anthropic":
      return `${base.replace(/\/v1$/, "")}/v1/messages`;
    case "google":
      return `${base.replace(/\/v1beta\/openai$/, "").replace(/\/openai$/, "")}/v1beta/interactions`;
    case "azure":
      return `${base.replace(/\/openai$/, "").replace(/\/v1$/, "")}/openai/…`;
    case "ollama":
      return `${base.replace(/\/v1$/, "")}/v1/chat/completions`;
    default: {
      const norm =
        /\/(v1|v3|v4|openai)$/.test(base) ||
        base.includes("/paas/v4") ||
        base.includes("/v1beta/openai")
          ? base
          : `${base}/v1`;
      return `${norm}/chat/completions`;
    }
  }
}

/** 列表状态点：未启动灰 / 健康绿 / 不健康红 */
type HealthStatus = "ok" | "fail" | "checking";

/** Providers 面板入参 */
type Props = {
  /** 面板是否可见 */
  active: boolean;
  /** 配置变更后回传完整状态（供 App 同步 ModelPicker 等） */
  onStateChange?: (state: ProvidersStateDto) => void;
  tone?: string;
};

/**
 * 添加提供商只列支持 Responses API 的内置项：
 * 不支持的（anthropic / google / zhipu / ollama / nvidia / moonshot / volcengine / hunyuan）
 * 既不能用于 Agent 对话，也不再提供入口；自定义提供商按 OpenAI 兼容处理，保留。
 */
const ADD_KINDS: ProviderKindId[] = [
  "openai",
  "deepseek",
  "azure",
  "openrouter",
  "bailian",
  "minimax",
  "custom",
];

/** 是否运行在 Tauri 壳内 */
function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** 供应商 kind → i18n key */
function kindLabelKey(kind: string): MessageKey {
  const key = `providers.kind.${kind}` as MessageKey;
  return key;
}

/** 编辑中的供应商草稿字段 */
type Draft = {
  display_name: string;
  endpoint: string;
  model: string;
  enabled: boolean;
  /** 聊天后备链（最多 3） */
  fallback: ProviderFallbackEntry[];
  image_model: string;
  video_model: string;
  tts_model: string;
  music_model: string;
  vision_model: string;
  embedding_model: string;
};

type DetailTab = "chat" | "models" | "media" | "voice" | "embedding";

type PageTab = "providers" | "auxiliary";

const MEDIA_MODEL_DEFAULTS: Record<
  string,
  { image: string; video: string; tts: string; music: string; vision: string }
> = {
  google: {
    image: "nano-banana-pro-preview",
    video: "veo-3.1-generate-preview",
    tts: "gemini-3.1-flash-tts-preview",
    music: "lyria-3-clip-preview",
    vision: "gemini-3.5-flash",
  },
  openai: {
    image: "gpt-image-2.5-flare",
    video: "",
    tts: "gpt-4o-mini-tts",
    music: "",
    vision: "gpt-4o",
  },
  azure: {
    image: "gpt-image-2.5-flare",
    video: "",
    tts: "",
    music: "",
    vision: "",
  },
  minimax: {
    image: "image-01",
    video: "MiniMax-H3",
    tts: "speech-2.8-hd",
    music: "music-3.0",
    vision: "",
  },
};

function supportsMediaModels(p: ProviderDto): boolean {
  return !!(
    p.supports_image ||
    p.supports_video ||
    p.supports_tts ||
    p.supports_music ||
    p.supports_asr
  );
}

function supportsVoice(p: ProviderDto): boolean {
  return !!(p.supports_tts || p.supports_asr);
}

function supportsEmbedding(p: ProviderDto): boolean {
  return !!p.supports_embedding;
}

function draftFromProvider(p: ProviderDto): Draft {
  return {
    display_name: p.display_name,
    endpoint: p.endpoint,
    model: p.model,
    enabled: p.enabled,
    fallback: normalizeFallback(p.fallback),
    image_model: p.image_model?.trim() ?? "",
    video_model: p.video_model?.trim() ?? "",
    tts_model: p.tts_model?.trim() ?? "",
    music_model: p.music_model?.trim() ?? "",
    vision_model: p.vision_model?.trim() ?? "",
    embedding_model: p.embedding_model?.trim() ?? "",
  };
}

function providerSaveInput(
  selected: ProviderDto,
  draft: Draft,
  overrides?: { enabled?: boolean; model?: string },
) {
  return {
    id: selected.id,
    kind: selected.kind,
    display_name: draft.display_name.trim() || selected.display_name,
    endpoint: draft.endpoint.trim() || selected.endpoint,
    model: (overrides?.model ?? draft.model).trim() || selected.model,
    enabled: overrides?.enabled ?? draft.enabled,
    fallback: normalizeFallback(draft.fallback),
    image_model: draft.image_model.trim(),
    video_model: draft.video_model.trim(),
    tts_model: draft.tts_model.trim(),
    music_model: draft.music_model.trim(),
    vision_model: draft.vision_model.trim(),
    embedding_model: draft.embedding_model.trim(),
  };
}

/** 模型列表探测延迟结果 */
type ModelLatency = {
  ok: boolean;
  latency_ms: number;
};

/** 连通性检测图标 */
function IconStethoscope(props: SVGProps<SVGSVGElement>) {
  return <Stethoscope size={16} strokeWidth={2} aria-hidden {...props} />;
}

/** 搜索图标 */
function IconSearch(props: SVGProps<SVGSVGElement>) {
  return <Search size={16} strokeWidth={2} aria-hidden {...props} />;
}

/** 按新上架排序 */
function IconSortNewest(props: SVGProps<SVGSVGElement>) {
  return <ArrowUpDown size={16} strokeWidth={2} aria-hidden {...props} />;
}

/** 本周新增筛选 */
function IconThisWeek(props: SVGProps<SVGSVGElement>) {
  return <Sparkles size={16} strokeWidth={2} aria-hidden {...props} />;
}

/** 模型详情展开 */
function IconInfo(props: SVGProps<SVGSVGElement>) {
  return <Info size={16} strokeWidth={2} aria-hidden {...props} />;
}

/** 密钥可见性图标（显示 / 隐藏之间 morph） */
function IconKeyVisibility({ visible }: { visible: boolean }) {
  return (
    <MorphToggleIcon
      active={visible}
      activeIcon={EyeOffData}
      inactiveIcon={EyeData}
      size={14}
      strokeWidth={2}
      aria-hidden
    />
  );
}

/** 掩码展示 API Key（保留首尾若干字符） */
function maskApiKey(key: string): string {
  const trimmed = key.trim();
  if (!trimmed) return "";
  if (trimmed.length <= 8) return "•".repeat(trimmed.length);
  const head = trimmed.slice(0, 4);
  const tail = trimmed.slice(-4);
  const mid = "•".repeat(Math.min(16, Math.max(4, trimmed.length - 8)));
  return `${head}${mid}${tail}`;
}

/** 刷新模型列表 */
function IconRefresh(props: SVGProps<SVGSVGElement>) {
  return <RefreshCw size={16} strokeWidth={2} aria-hidden {...props} />;
}

/** 保存 */
function IconSave(props: SVGProps<SVGSVGElement>) {
  return <Save size={16} strokeWidth={2} aria-hidden {...props} />;
}

/** 设为默认 */
function IconStar(props: SVGProps<SVGSVGElement>) {
  return <Star size={16} strokeWidth={2} aria-hidden {...props} />;
}

/** 删除供应商 */
function IconTrash(props: SVGProps<SVGSVGElement>) {
  return <Trash2 size={16} strokeWidth={2} aria-hidden {...props} />;
}

/** 加载中 */
function IconLoader(props: SVGProps<SVGSVGElement>) {
  return <LoaderCircle size={16} strokeWidth={2} aria-hidden {...props} />;
}

/** 新增供应商 */
function IconPlus(props: SVGProps<SVGSVGElement>) {
  return <Plus size={16} strokeWidth={2} aria-hidden {...props} />;
}

/** 拖拽排序手柄 */
function IconGrip(props: SVGProps<SVGSVGElement>) {
  return <GripVertical size={14} strokeWidth={2} aria-hidden {...props} />;
}

function IconTag(props: SVGProps<SVGSVGElement>) {
  return <Tag size={12} strokeWidth={2} aria-hidden {...props} />;
}

function IconBox(props: SVGProps<SVGSVGElement>) {
  return <Box size={12} strokeWidth={2} aria-hidden {...props} />;
}

function IconLink(props: SVGProps<SVGSVGElement>) {
  return <Link2 size={12} strokeWidth={2} aria-hidden {...props} />;
}

function IconWaypoints(props: SVGProps<SVGSVGElement>) {
  return <Waypoints size={14} strokeWidth={2} aria-hidden {...props} />;
}

function IconKey(props: SVGProps<SVGSVGElement>) {
  return <KeyRound size={14} strokeWidth={2} aria-hidden {...props} />;
}

function IconLayers(props: SVGProps<SVGSVGElement>) {
  return <Layers size={14} strokeWidth={2} aria-hidden {...props} />;
}

export default function ProvidersPanel({ active, onStateChange, tone }: Props) {
  const { t } = useI18n();
  const confirm = useConfirm();
  const addKindOptions = useMemo(
    () =>
      ADD_KINDS.map((k) => ({
        value: k,
        label: t(kindLabelKey(k)),
        icon: <ProviderBrandIcon kind={k} />,
      })),
    [t],
  );
  const [state, setState] = useState<ProvidersStateDto | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [detailTab, setDetailTab] = useState<DetailTab>("chat");
  const [pageTab, setPageTab] = useState<PageTab>("providers");
  const [apiKeyInput, setApiKeyInput] = useState("");
  const [storedApiKey, setStoredApiKey] = useState<string | null>(null);
  const [showApiKey, setShowApiKey] = useState(false);
  const [apiKeyDirty, setApiKeyDirty] = useState(false);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [providerActionsOpen, setProviderActionsOpen] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testingAll, setTestingAll] = useState(false);
  const [listingModelsRequest, setListingModelsRequest] = useState<{
    providerId: string;
    requestId: number;
  } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [addKind, setAddKind] = useState<ProviderKindId>("openai");
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [sanitizeModelsProviderId, setSanitizeModelsProviderId] = useState<
    string | null
  >(null);
  const [modelsLatency, setModelsLatency] = useState<number | null>(null);
  const [modelLatencies, setModelLatencies] = useState<
    Record<string, ModelLatency>
  >({});
  const [testResult, setTestResult] = useState<ProviderTestResult | null>(null);
  const [modelFilter, setModelFilter] = useState("");
  const [showFilter, setShowFilter] = useState(false);
  /** 按 OpenRouter created 降序（新上架优先）；默认开启 */
  const [sortByNewest, setSortByNewest] = useState(true);
  /** 仅显示近 7 天内上架的模型 */
  const [onlyThisWeek, setOnlyThisWeek] = useState(false);
  /** 展开详情的模型 id */
  const [expandedModelId, setExpandedModelId] = useState<string | null>(null);
  const [showAddModel, setShowAddModel] = useState(false);
  const [customModelInput, setCustomModelInput] = useState("");
  const [drag, setDrag] = useState<{
    id: string;
    fromIndex: number;
    insertIndex: number;
    x: number;
    y: number;
    width: number;
    height: number;
    offsetX: number;
    offsetY: number;
  } | null>(null);
  const [healthById, setHealthById] = useState<Record<string, HealthStatus>>(
    {},
  );
  /** 后备供应商 → 可选模型列表（缓存 / 拉取） */
  const [fallbackModelsById, setFallbackModelsById] = useState<
    Record<string, ModelInfo[]>
  >({});
  const autoFetchIdRef = useRef<string | null>(null);
  const selectedIdRef = useRef(selectedId);
  const modelsRequestRef = useRef(0);
  const healthRunRef = useRef(0);
  const listRef = useRef<HTMLUListElement | null>(null);
  const providerActionsAnchorRef = useRef<HTMLButtonElement | null>(null);
  const dragRef = useRef(drag);
  dragRef.current = drag;
  selectedIdRef.current = selectedId;

  const applyState = useCallback(
    (next: ProvidersStateDto) => {
      setState(next);
      onStateChange?.(next);
    },
    [onStateChange],
  );

  const checkProviderHealth = useCallback(
    async (provider: ProviderDto, runId?: number) => {
      if (!provider.enabled) return;
      // 需要密钥却未配置 → 视为健康检查失败
      if (provider.kind !== "ollama" && !provider.has_api_key) {
        if (runId !== undefined && runId !== healthRunRef.current) return;
        setHealthById((prev) => ({ ...prev, [provider.id]: "fail" }));
        return;
      }
      setHealthById((prev) => ({ ...prev, [provider.id]: "checking" }));
      try {
        // 用拉取模型列表做轻量连通性探测（不消耗生成额度）
        await invoke<ProviderModelsResult>("list_provider_models", {
          id: provider.id,
        });
        // 关闭后忽略过期结果，避免又把灰点刷成红/绿
        if (runId !== undefined && runId !== healthRunRef.current) return;
        setHealthById((prev) => ({ ...prev, [provider.id]: "ok" }));
      } catch {
        if (runId !== undefined && runId !== healthRunRef.current) return;
        setHealthById((prev) => ({ ...prev, [provider.id]: "fail" }));
      }
    },
    [],
  );

  const runHealthChecks = useCallback(
    (providers: ProviderDto[]) => {
      const runId = ++healthRunRef.current;
      const enabled = providers.filter((p) => p.enabled);
      // 未启动的清掉健康状态，避免残留绿/红点
      setHealthById((prev) => {
        const next = { ...prev };
        for (const p of providers) {
          if (!p.enabled) delete next[p.id];
        }
        return next;
      });
      void Promise.all(
        enabled.map(async (p) => {
          if (runId !== healthRunRef.current) return;
          await checkProviderHealth(p, runId);
        }),
      );
    },
    [checkProviderHealth],
  );

  const refresh = useCallback(async () => {
    if (!isTauri()) return;
    setLoading(true);
    setError(null);
    try {
      const next = await invoke<ProvidersStateDto>("get_providers_state");
      applyState(next);
      setSelectedId((prev) => {
        if (prev && next.providers.some((p) => p.id === prev)) return prev;
        return next.active_provider_id ?? next.providers[0]?.id ?? null;
      });
      runHealthChecks(next.providers);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, [applyState, runHealthChecks]);

  useEffect(() => {
    if (!active) return;
    void refresh();
  }, [active, refresh]);

  const statusDotClass = (enabled: boolean, health?: HealthStatus) => {
    if (!enabled) return "off";
    if (health === "ok") return "ok";
    if (health === "fail") return "fail";
    return "checking";
  };

  const statusDotTitle = (enabled: boolean, health?: HealthStatus) => {
    if (!enabled) return t("providers.statusOff");
    if (health === "ok") return t("providers.statusHealthy");
    if (health === "fail") return t("providers.statusUnhealthy");
    return t("providers.statusChecking");
  };

  const selected = state?.providers.find((p) => p.id === selectedId) ?? null;
  const hasUnsavedChanges = useMemo(() => {
    if (!selected || !draft) return false;
    return (
      JSON.stringify(providerSaveInput(selected, draft)) !==
      JSON.stringify(providerSaveInput(selected, draftFromProvider(selected)))
    );
  }, [draft, selected]);
  const listingModels = isMediaModelsRequestLoading(
    selected?.id ?? null,
    modelsRequestRef.current,
    listingModelsRequest,
  );

  useEffect(() => {
    if (!selected) {
      setDraft(null);
      setSanitizeModelsProviderId(null);
      modelsRequestRef.current += 1;
      setListingModelsRequest(null);
      setStoredApiKey(null);
      setApiKeyInput("");
      setShowApiKey(false);
      setApiKeyDirty(false);
      autoFetchIdRef.current = null;
      return;
    }
    setDraft(draftFromProvider(selected));
    setSanitizeModelsProviderId(null);
    modelsRequestRef.current += 1;
    setListingModelsRequest(null);
    setDetailTab("chat");
    setApiKeyInput("");
    setStoredApiKey(null);
    setShowApiKey(false);
    setApiKeyDirty(false);
    setModels([]);
    setModelsLatency(null);
    setModelLatencies({});
    setTestResult(null);
    setModelFilter("");
    setShowFilter(false);
    setSortByNewest(true);
    setOnlyThisWeek(false);
    setExpandedModelId(null);
    setShowAddModel(false);
    setCustomModelInput("");
    setProviderActionsOpen(false);
    setError(null);
    autoFetchIdRef.current = null;

    if (!isTauri() || selected.kind === "ollama" || !selected.has_api_key) {
      return;
    }
    const id = selected.id;
    void invoke<string | null>("get_provider_api_key", { id })
      .then((key) => {
        setStoredApiKey(key && key.trim() ? key : null);
      })
      .catch(() => {
        setStoredApiKey(null);
      });
  }, [selected?.id, selected?.has_api_key, selected?.kind]);

  // 切到不支持媒体的提供商时，避免停在空的媒体页
  useEffect(() => {
    if (detailTab === "media" && selected && !supportsMediaModels(selected)) {
      setDetailTab("chat");
    }
    if (detailTab === "voice" && selected && !supportsVoice(selected)) {
      setDetailTab("chat");
    }
    if (detailTab === "embedding" && selected && !supportsEmbedding(selected)) {
      setDetailTab("chat");
    }
  }, [detailTab, selected]);

  // selected 字段外部更新时同步草稿（不重置模型列表）
  useEffect(() => {
    if (!selected) return;
    setDraft(draftFromProvider(selected));
  }, [
    selected?.display_name,
    selected?.endpoint,
    selected?.model,
    selected?.enabled,
    selected?.fallback,
    selected?.image_model,
    selected?.video_model,
    selected?.tts_model,
    selected?.music_model,
    selected?.vision_model,
    selected?.embedding_model,
  ]);

  const mediaOptions = useCallback(
    (capability: MediaCapabilityKey, defaultModelId: string) =>
      buildMediaModelOptions(models, capability, defaultModelId).map(
        (option) => ({
          value: option.value,
          label:
            option.value === ""
              ? capability === "image_gen" &&
                ["openai", "azure"].includes(selected?.kind ?? "")
                ? t("providers.imageSceneAutoOption")
                : t("providers.mediaDefaultOption", { model: option.modelId })
              : option.modelId,
          icon: option.value ? (
            <ModelBrandIcon modelId={option.modelId} />
          ) : undefined,
        }),
      ),
    [models, t, selected?.kind],
  );

  useEffect(() => {
    if (sanitizeModelsProviderId !== selected?.id || !selected || !draft)
      return;
    const defaults = MEDIA_MODEL_DEFAULTS[selected.kind];
    if (!defaults) return;
    setDraft((current) => {
      if (!current) return current;
      const next = {
        ...current,
        image_model: sanitizeMediaModelValue(
          current.image_model,
          buildMediaModelOptions(models, "image_gen", defaults.image),
        ),
        tts_model: sanitizeMediaModelValue(
          current.tts_model,
          buildMediaModelOptions(models, "audio_gen", defaults.tts),
        ),
        vision_model: sanitizeMediaModelValue(
          current.vision_model,
          buildMediaModelOptions(models, "vision", defaults.vision),
        ),
        video_model: selected.supports_video
          ? sanitizeMediaModelValue(
              current.video_model,
              buildMediaModelOptions(models, "video_gen", defaults.video),
            )
          : "",
        music_model: selected.supports_music
          ? sanitizeMediaModelValue(
              current.music_model,
              buildMediaModelOptions(models, "music_gen", defaults.music),
            )
          : "",
      };
      return JSON.stringify(next) === JSON.stringify(current) ? current : next;
    });
  }, [models, sanitizeModelsProviderId, selected?.id, selected?.kind]);

  const saveDraft = async () => {
    if (!selected || !draft || !isTauri()) return;
    setSaving(true);
    setError(null);
    try {
      const next = await invoke<ProvidersStateDto>("save_provider", {
        provider: providerSaveInput(selected, draft),
      });
      applyState(next);
      const saved = next.providers.find((p) => p.id === selected.id);
      if (saved?.enabled) {
        const runId = ++healthRunRef.current;
        void checkProviderHealth(saved, runId);
      } else if (saved) {
        healthRunRef.current += 1;
        setHealthById((prev) => {
          const copy = { ...prev };
          delete copy[saved.id];
          return copy;
        });
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  };

  /** 启用开关立即落盘，列表状态点同步变灰/重检 */
  const toggleEnabled = async () => {
    if (!selected || !draft || !isTauri()) return;
    const nextEnabled = !draft.enabled;
    setDraft((d) => (d ? { ...d, enabled: nextEnabled } : d));
    // 先乐观更新列表点：关闭立刻变灰，避免仍显示红点
    if (!nextEnabled) {
      // 作废进行中的健康检查，防止异步结果把灰点刷回红/绿
      healthRunRef.current += 1;
      setHealthById((prev) => {
        const copy = { ...prev };
        delete copy[selected.id];
        return copy;
      });
      setState((prev) => {
        if (!prev) return prev;
        return {
          ...prev,
          providers: prev.providers.map((p) =>
            p.id === selected.id ? { ...p, enabled: false } : p,
          ),
        };
      });
    } else {
      setState((prev) => {
        if (!prev) return prev;
        return {
          ...prev,
          providers: prev.providers.map((p) =>
            p.id === selected.id ? { ...p, enabled: true } : p,
          ),
        };
      });
      setHealthById((prev) => ({ ...prev, [selected.id]: "checking" }));
    }
    setSaving(true);
    setError(null);
    try {
      const next = await invoke<ProvidersStateDto>("save_provider", {
        provider: providerSaveInput(selected, draft, { enabled: nextEnabled }),
      });
      applyState(next);
      const saved = next.providers.find((p) => p.id === selected.id);
      if (saved?.enabled) {
        const runId = ++healthRunRef.current;
        void checkProviderHealth(saved, runId);
      } else if (saved) {
        setHealthById((prev) => {
          const copy = { ...prev };
          delete copy[saved.id];
          return copy;
        });
      }
    } catch (err) {
      // 回滚乐观更新
      setDraft((d) => (d ? { ...d, enabled: !nextEnabled } : d));
      setError(String(err));
      void refresh();
    } finally {
      setSaving(false);
    }
  };

  const setActive = async (id: string) => {
    if (!isTauri()) return;
    setError(null);
    try {
      const next = await invoke<ProvidersStateDto>("set_active_provider", {
        id,
      });
      applyState(next);
    } catch (err) {
      setError(String(err));
    }
  };

  const setImageActive = async (id: string) => {
    if (!isTauri()) return;
    setError(null);
    try {
      const next = await invoke<ProvidersStateDto>(
        "set_active_image_provider",
        { id },
      );
      applyState(next);
    } catch (err) {
      setError(String(err));
    }
  };

  const addProvider = async () => {
    if (!isTauri()) return;
    setError(null);
    try {
      const next = await invoke<ProvidersStateDto>("add_provider", {
        kind: addKind,
      });
      applyState(next);
      const added = next.providers[next.providers.length - 1];
      if (added) setSelectedId(added.id);
    } catch (err) {
      setError(String(err));
    }
  };

  const reorderProviders = async (fromIndex: number, toIndex: number) => {
    if (!state || fromIndex === toIndex) return;
    const ids = state.providers.map((p) => p.id);
    if (
      fromIndex < 0 ||
      toIndex < 0 ||
      fromIndex >= ids.length ||
      toIndex >= ids.length
    ) {
      return;
    }
    const nextIds = [...ids];
    const [moved] = nextIds.splice(fromIndex, 1);
    nextIds.splice(toIndex, 0, moved);
    const byId = new Map(state.providers.map((p) => [p.id, p]));
    const optimistic: ProvidersStateDto = {
      ...state,
      providers: nextIds
        .map((id) => byId.get(id))
        .filter((p): p is ProviderDto => !!p),
    };
    applyState(optimistic);
    if (!isTauri()) return;
    try {
      const next = await invoke<ProvidersStateDto>("reorder_providers", {
        ids: nextIds,
      });
      applyState(next);
    } catch (err) {
      setError(String(err));
      void refresh();
    }
  };

  const calcInsertIndex = useCallback((clientY: number) => {
    const list = listRef.current;
    if (!list) return 0;
    const items = Array.from(
      list.querySelectorAll<HTMLElement>("[data-provider-id]"),
    );
    if (items.length === 0) return 0;
    for (let i = 0; i < items.length; i++) {
      const rect = items[i].getBoundingClientRect();
      if (clientY < rect.top + rect.height / 2) return i;
    }
    return items.length - 1;
  }, []);

  const onDragHandlePointerDown = (
    e: ReactPointerEvent<HTMLSpanElement>,
    provider: ProviderDto,
    index: number,
  ) => {
    if (e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    const row = (e.currentTarget.closest("li") as HTMLElement | null) ?? null;
    const rect = (row ?? e.currentTarget).getBoundingClientRect();
    setDrag({
      id: provider.id,
      fromIndex: index,
      insertIndex: index,
      x: e.clientX,
      y: e.clientY,
      width: rect.width,
      height: rect.height,
      offsetX: e.clientX - rect.left,
      offsetY: e.clientY - rect.top,
    });
  };

  useEffect(() => {
    if (!drag) return;
    const onMove = (e: PointerEvent) => {
      const cur = dragRef.current;
      if (!cur) return;
      e.preventDefault();
      setDrag({
        ...cur,
        x: e.clientX,
        y: e.clientY,
        insertIndex: calcInsertIndex(e.clientY),
      });
    };
    const onUp = () => {
      const cur = dragRef.current;
      setDrag(null);
      if (!cur) return;
      let to = cur.insertIndex;
      if (cur.fromIndex < to) to -= 1;
      if (to !== cur.fromIndex) {
        void reorderProviders(cur.fromIndex, to);
        const el = listRef.current;
        if (el) {
          el.classList.add("is-settled");
          const onEnd = () => {
            el.classList.remove("is-settled");
            el.removeEventListener("animationend", onEnd);
          };
          el.addEventListener("animationend", onEnd);
        }
      }
    };
    window.addEventListener("pointermove", onMove, { passive: false });
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onUp);
    return () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onUp);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 拖拽开始时绑定一次
  }, [!!drag, calcInsertIndex]);

  const deleteProvider = async () => {
    if (!selected) return;
    if (!isTauri()) {
      setError("请在桌面应用中删除提供商");
      return;
    }
    const ok = await confirm({
      title: t("dialog.deleteTitle"),
      message: `${t("providers.delete")} — ${selected.display_name}?`,
      confirmLabel: t("providers.delete"),
      variant: "danger",
    });
    if (!ok) return;
    const deletingId = selected.id;
    setError(null);
    try {
      const next = await invoke<ProvidersStateDto>("delete_provider", {
        id: deletingId,
      });
      applyState(next);
      const stillThere = next.providers.some((p) => p.id === deletingId);
      if (stillThere) {
        setError("删除未生效，请重试");
        return;
      }
      setSelectedId(next.active_provider_id ?? next.providers[0]?.id ?? null);
    } catch (err) {
      setError(String(err));
    }
  };

  const saveApiKey = async () => {
    if (!selected || !isTauri()) return;
    const nextKey = apiKeyInput.trim();
    if (!nextKey) return;
    setSaving(true);
    setError(null);
    try {
      const next = await invoke<ProvidersStateDto>("set_provider_api_key", {
        id: selected.id,
        apiKey: nextKey,
      });
      applyState(next);
      setStoredApiKey(nextKey);
      setApiKeyInput("");
      setApiKeyDirty(false);
      setShowApiKey(false);
      const saved = next.providers.find((p) => p.id === selected.id);
      if (saved?.enabled) {
        const runId = ++healthRunRef.current;
        void checkProviderHealth(saved, runId);
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  };

  const clearApiKey = async () => {
    if (!selected || !isTauri()) return;
    const confirmed = await confirm({
      title: t("providers.clearKeyTitle"),
      message: t("providers.clearKeyConfirm"),
      confirmLabel: t("providers.clearKey"),
      variant: "danger",
    });
    if (!confirmed) return;
    setError(null);
    try {
      const next = await invoke<ProvidersStateDto>("clear_provider_api_key", {
        id: selected.id,
      });
      applyState(next);
      setApiKeyInput("");
      setStoredApiKey(null);
      setApiKeyDirty(false);
      setShowApiKey(false);
      const saved = next.providers.find((p) => p.id === selected.id);
      if (saved?.enabled) {
        const runId = ++healthRunRef.current;
        void checkProviderHealth(saved, runId);
      } else {
        healthRunRef.current += 1;
        setHealthById((prev) => {
          const copy = { ...prev };
          delete copy[selected.id];
          return copy;
        });
      }
    } catch (err) {
      setError(String(err));
    }
  };

  const listModels = async (opts?: {
    silent?: boolean;
    skipSave?: boolean;
    provider?: ProviderDto;
    requestId?: number;
  }) => {
    const provider = opts?.provider ?? selected;
    if (!provider || !isTauri()) return;
    const requestId = opts?.requestId ?? ++modelsRequestRef.current;
    const isCurrentRequest = () =>
      evaluateMediaModelsResult(
        selectedIdRef.current,
        modelsRequestRef.current,
        provider.id,
        requestId,
        "online-failure",
      ).accept;
    if (!isCurrentRequest()) return;
    const silent = opts?.silent ?? false;
    setListingModelsRequest({ providerId: provider.id, requestId });
    if (!silent) setError(null);
    try {
      // 手动拉取时先落盘草稿；自动拉取跳过，避免切提供商时用到旧草稿
      if (draft && !opts?.skipSave) {
        await invoke<ProvidersStateDto>("save_provider", {
          provider: providerSaveInput(provider, draft),
        }).then(applyState);
      }
      const result = await invoke<ProviderModelsResult>(
        "list_provider_models",
        {
          id: provider.id,
        },
      );
      const decision = evaluateMediaModelsResult(
        selectedIdRef.current,
        modelsRequestRef.current,
        provider.id,
        requestId,
        "online-success",
      );
      if (!decision.accept) return;
      setModels(result.models);
      if (decision.sanitize) setSanitizeModelsProviderId(provider.id);
      setModelsLatency(result.latency_ms);
      setModelLatencies({});
      if (provider.enabled) {
        setHealthById((prev) => ({ ...prev, [provider.id]: "ok" }));
      }
    } catch (err) {
      if (!isCurrentRequest()) return;
      if (!silent) setError(String(err));
      if (provider.enabled) {
        setHealthById((prev) => ({ ...prev, [provider.id]: "fail" }));
      }
    } finally {
      if (isCurrentRequest()) {
        setListingModelsRequest((current) =>
          current?.providerId === provider.id && current.requestId === requestId
            ? null
            : current,
        );
      }
    }
  };

  // 选中提供商后：先读 models.json 缓存，再自动拉取并回写
  useEffect(() => {
    if (!active || !selected || !isTauri()) return;
    const canList = selected.kind === "ollama" || selected.has_api_key;
    if (!canList) {
      autoFetchIdRef.current = null;
      return;
    }
    if (autoFetchIdRef.current === selected.id) return;
    autoFetchIdRef.current = selected.id;
    const provider = selected;
    const requestId = ++modelsRequestRef.current;
    void (async () => {
      try {
        const cached = await invoke<ProviderModelsResult | null>(
          "get_cached_provider_models",
          { id: provider.id },
        );
        const decision = evaluateMediaModelsResult(
          selectedIdRef.current,
          modelsRequestRef.current,
          provider.id,
          requestId,
          "cache",
        );
        if (decision.showModels && cached && cached.models.length > 0) {
          setModels(cached.models);
          setModelsLatency(cached.latency_ms);
        }
      } catch {
        // ignore cache miss
      }
      await listModels({
        silent: true,
        skipSave: true,
        provider,
        requestId,
      });
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 仅在切换提供商 / 密钥就绪时自动拉取
  }, [active, selected?.id, selected?.has_api_key, selected?.kind]);

  const testConnection = async (modelOverride?: string) => {
    if (!selected || !isTauri()) return;
    setTesting(true);
    setError(null);
    setTestResult(null);
    const modelId = modelOverride ?? draft?.model ?? selected.model;
    try {
      if (draft) {
        await invoke<ProvidersStateDto>("save_provider", {
          provider: providerSaveInput(selected, draft, { model: modelId }),
        }).then(applyState);
      }
      const result = await invoke<ProviderTestResult>("test_provider", {
        id: selected.id,
        model: modelId,
      });
      setTestResult(result);
      setModelLatencies((prev) => ({
        ...prev,
        [result.model]: { ok: result.ok, latency_ms: result.latency_ms },
      }));
      if (selected.enabled || draft?.enabled) {
        setHealthById((prev) => ({
          ...prev,
          [selected.id]: result.ok ? "ok" : "fail",
        }));
      }
      if (!result.ok) {
        setError(result.message);
      }
    } catch (err) {
      setError(String(err));
      if (selected.enabled || draft?.enabled) {
        setHealthById((prev) => ({ ...prev, [selected.id]: "fail" }));
      }
    } finally {
      setTesting(false);
    }
  };

  const testAllModels = async () => {
    if (!selected || !isTauri() || models.length === 0) return;
    setTestingAll(true);
    setError(null);
    // 先清空旧延时，批量结果回来后一次性写入
    setModelLatencies({});
    try {
      const results = await invoke<ProviderTestResult[]>(
        "test_provider_models",
        {
          id: selected.id,
          models: models.map((m) => m.id),
        },
      );
      const next: Record<string, ModelLatency> = {};
      for (const result of results) {
        next[result.model] = { ok: result.ok, latency_ms: result.latency_ms };
      }
      // 未返回的模型标为失败（理论上不应发生）
      for (const model of models) {
        if (!(model.id in next)) {
          next[model.id] = { ok: false, latency_ms: -1 };
        }
      }
      setModelLatencies(next);
      if (selected.enabled || draft?.enabled) {
        const anyOk = results.some((r) => r.ok);
        setHealthById((prev) => ({
          ...prev,
          [selected.id]: anyOk ? "ok" : "fail",
        }));
      }
    } catch (err) {
      setError(String(err));
      setModelLatencies(
        Object.fromEntries(
          models.map((m) => [m.id, { ok: false, latency_ms: -1 }]),
        ),
      );
      if (selected.enabled || draft?.enabled) {
        setHealthById((prev) => ({ ...prev, [selected.id]: "fail" }));
      }
    } finally {
      setTestingAll(false);
    }
  };

  const useModel = async (
    modelId: string,
    expirationDate?: string | null,
  ): Promise<boolean> => {
    const exp = expirationDate?.trim();
    if (exp) {
      const ok = await confirm({
        title: t("providers.expiringConfirmTitle"),
        message: t("providers.expiringConfirmMessage"),
        emphasis: modelId,
        emphasisLabel: `${t("providers.expiration")}: ${exp}`,
        confirmLabel: t("providers.expiringConfirmOk"),
        variant: "danger",
      });
      if (!ok) return false;
    }
    setDraft((d) => (d ? { ...d, model: modelId } : d));
    return true;
  };

  const addCustomModel = () => {
    const id = customModelInput.trim();
    if (!id) return;
    setModels((prev) =>
      prev.some((m) => m.id === id)
        ? prev
        : [
            {
              id,
              capabilities: {
                vision: false,
                web: false,
                reasoning: false,
                tools: true,
                file: false,
                audio_in: false,
                image_gen: false,
                video_gen: false,
                audio_gen: false,
                music_gen: false,
              },
              meta_source: "manual",
            },
            ...prev,
          ],
    );
    void useModel(id);
    setCustomModelInput("");
    setShowAddModel(false);
  };

  const openOfficialKey = async () => {
    const url = selected?.official_key_url;
    if (!url) return;
    try {
      if (isTauri()) {
        await shellOpen(url);
      } else {
        window.open(url, "_blank", "noopener,noreferrer");
      }
    } catch {
      window.open(url, "_blank", "noopener,noreferrer");
    }
  };

  const enabledCount = state?.providers.filter((p) => p.enabled).length ?? 0;
  const total = state?.providers.length ?? 0;
  const isActive = state?.active_provider_id === selected?.id;
  const isImageActive = state?.active_image_provider_id === selected?.id;
  const needsKey = selected?.kind !== "ollama";
  const filteredModels = useMemo(() => {
    const q = modelFilter.trim().toLowerCase();
    let list = models.filter((m) => {
      if (onlyThisWeek && !isModelCreatedWithinDays(m.created, 7)) return false;
      if (!q) return true;
      return (
        m.id.toLowerCase().includes(q) ||
        (m.display_name?.toLowerCase().includes(q) ?? false)
      );
    });
    if (sortByNewest) {
      list = [...list].sort(compareModelsByCreatedDesc);
    }
    return list;
  }, [modelFilter, models, onlyThisWeek, sortByNewest]);
  const apiKeyDisplayValue = apiKeyDirty
    ? apiKeyInput
    : storedApiKey
      ? showApiKey
        ? storedApiKey
        : maskApiKey(storedApiKey)
      : apiKeyInput;
  const canSaveApiKey = apiKeyDirty && apiKeyInput.trim().length > 0;

  const fallbackEntries = draft?.fallback ?? [];
  const fallbackCandidateProviders =
    state?.providers.filter(
      (p) =>
        p.enabled &&
        p.supports_responses_api === true &&
        p.id !== selected?.id &&
        !fallbackEntries.some((f) => f.provider_id === p.id),
    ) ?? [];

  const addFallback = (providerId: string) => {
    if (!providerId || fallbackEntries.length >= MAX_MODEL_FALLBACKS) return;
    setDraft((d) =>
      d
        ? {
            ...d,
            fallback: [
              ...d.fallback,
              { provider_id: providerId, model: null },
            ].slice(0, MAX_MODEL_FALLBACKS),
          }
        : d,
    );
  };

  const removeFallback = (index: number) => {
    setDraft((d) =>
      d ? { ...d, fallback: d.fallback.filter((_, i) => i !== index) } : d,
    );
  };

  const updateFallbackModel = (index: number, model: string) => {
    setDraft((d) => {
      if (!d) return d;
      const trimmed = model.trim();
      const next = d.fallback.map((entry, i) =>
        i === index ? { ...entry, model: trimmed ? trimmed : null } : entry,
      );
      return { ...d, fallback: next };
    });
  };

  const providerLabel = (id: string) => {
    const p = state?.providers.find((x) => x.id === id);
    return p?.display_name ?? id;
  };

  const fallbackProviderIds = fallbackEntries
    .map((e) => e.provider_id)
    .filter(Boolean)
    .sort()
    .join(",");

  // 为每条后备加载模型列表（优先缓存，必要时在线拉取）
  useEffect(() => {
    if (!active || !isTauri() || !fallbackProviderIds) {
      return;
    }
    const ids = [...new Set(fallbackProviderIds.split(",").filter(Boolean))];
    let cancelled = false;
    void (async () => {
      for (const id of ids) {
        if (cancelled) return;
        const provider = state?.providers.find((p) => p.id === id);
        if (!provider) continue;
        const canList = provider.kind === "ollama" || provider.has_api_key;
        if (!canList) {
          setFallbackModelsById((prev) =>
            prev[id] ? prev : { ...prev, [id]: [] },
          );
          continue;
        }
        let modelsForId: ModelInfo[] = [];
        try {
          const cached = await invoke<ProviderModelsResult | null>(
            "get_cached_provider_models",
            { id },
          );
          if (cached?.models?.length) {
            modelsForId = cached.models;
          }
        } catch {
          // ignore
        }
        if (!cancelled && modelsForId.length === 0) {
          try {
            const listed = await invoke<ProviderModelsResult>(
              "list_provider_models",
              { id },
            );
            modelsForId = listed.models ?? [];
          } catch {
            // ignore — 下拉仍可用供应商默认 model
          }
        }
        if (cancelled) return;
        setFallbackModelsById((prev) => {
          if (
            prev[id]?.length === modelsForId.length &&
            prev[id]?.every((m, i) => m.id === modelsForId[i]?.id)
          ) {
            return prev;
          }
          return { ...prev, [id]: modelsForId };
        });
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [active, fallbackProviderIds, state?.providers]);

  return (
    <div className="providers-page" data-tone={tone ?? "cyan"}>
      <div
        className="providers-page-tabs"
        role="tablist"
        aria-label={t("providers.pageTabs")}
      >
        <button
          type="button"
          role="tab"
          aria-selected={pageTab === "providers"}
          className={`providers-page-tab ${pageTab === "providers" ? "is-active" : ""}`}
          onClick={() => setPageTab("providers")}
        >
          <Zap size={14} strokeWidth={2.2} aria-hidden />
          {t("providers.tabProviders")}
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={pageTab === "auxiliary"}
          className={`providers-page-tab ${pageTab === "auxiliary" ? "is-active" : ""}`}
          onClick={() => setPageTab("auxiliary")}
        >
          <Brain size={14} strokeWidth={2.2} aria-hidden />
          {t("providers.tabAuxiliary")}
        </button>
      </div>

      {pageTab === "auxiliary" ? (
        <AuxiliaryModelsPanel active={active} embedded tone={tone} />
      ) : (
        <div className="providers-layout">
          <Surface
            variant="card"
            className="providers-pane providers-pane-list"
            role="complementary"
          >
            <div className="providers-pane-head">
              <h2>{t("providers.listTitle")}</h2>
              <p>
                {t("providers.listSub", {
                  count: String(enabledCount),
                  total: String(total),
                })}
              </p>
            </div>

            <ul
              className={`providers-list ${drag ? "is-reordering" : ""}`}
              ref={listRef}
            >
              {loading && !state && (
                <li className="providers-empty">{t("skills.refreshing")}</li>
              )}
              {!loading && state?.providers.length === 0 && (
                <li className="providers-empty">{t("providers.noneAdded")}</li>
              )}
              {state?.providers
                .slice()
                .sort((a, b) => {
                  const rank = (p: typeof a) => {
                    if (!p.enabled) return 2;
                    const h = healthById[p.id];
                    if (h === "ok") return 0;
                    return 1;
                  };
                  return rank(a) - rank(b);
                })
                .map((p, index) => {
                  const activeDefault = state.active_provider_id === p.id;
                  const isDragging = drag?.id === p.id;
                  const showInsertBefore =
                    !!drag &&
                    drag.insertIndex === index &&
                    drag.fromIndex !== drag.insertIndex;
                  return (
                    <li
                      key={p.id}
                      data-provider-id={p.id}
                      className={isDragging ? "is-dragging" : ""}
                    >
                      {showInsertBefore && (
                        <div className="providers-drop-indicator" aria-hidden />
                      )}
                      <button
                        type="button"
                        className={`providers-list-item ${selectedId === p.id ? "is-selected" : ""}`}
                        aria-pressed={selectedId === p.id}
                        onClick={() => {
                          if (drag) return;
                          setSelectedId(p.id);
                        }}
                      >
                        <span
                          className="providers-drag-handle"
                          title={t("providers.dragToReorder")}
                          aria-label={t("providers.dragToReorder")}
                          onPointerDown={(e) =>
                            onDragHandlePointerDown(e, p, index)
                          }
                          onClick={(e) => e.stopPropagation()}
                        >
                          <IconGrip />
                        </span>
                        <span className="providers-list-icon" aria-hidden>
                          <ProviderBrandIcon kind={p.kind} size={24} />
                        </span>
                        <span className="providers-list-text">
                          <span className="providers-list-name">
                            <span
                              className="providers-list-label"
                              title={p.display_name}
                            >
                              {p.display_name}
                            </span>
                            {activeDefault && (
                              <span
                                className="providers-badge"
                                title={t("providers.active")}
                              >
                                {t("providers.defaultBadge")}
                              </span>
                            )}
                          </span>
                          <span
                            className="providers-list-model"
                            title={p.model}
                          >
                            {p.model}
                          </span>
                        </span>
                        <span
                          className={`providers-status-dot ${statusDotClass(p.enabled, healthById[p.id])}`}
                          title={statusDotTitle(p.enabled, healthById[p.id])}
                          role="img"
                          aria-label={statusDotTitle(
                            p.enabled,
                            healthById[p.id],
                          )}
                        />
                      </button>
                    </li>
                  );
                })}
            </ul>

            {drag &&
              state &&
              createPortal(
                (() => {
                  const p = state.providers.find((x) => x.id === drag.id);
                  if (!p) return null;
                  const activeDefault = state.active_provider_id === p.id;
                  return (
                    <div
                      className="providers-drag-ghost"
                      aria-hidden
                      style={{
                        width: drag.width,
                        height: drag.height,
                        transform: `translate(${drag.x - drag.offsetX}px, ${drag.y - drag.offsetY}px)`,
                      }}
                    >
                      <div className="providers-list-item is-ghost">
                        <span className="providers-drag-handle is-visible">
                          <IconGrip />
                        </span>
                        <span className="providers-list-icon" aria-hidden>
                          <ProviderBrandIcon kind={p.kind} size={24} />
                        </span>
                        <span className="providers-list-text">
                          <span className="providers-list-name">
                            <span
                              className="providers-list-label"
                              title={p.display_name}
                            >
                              {p.display_name}
                            </span>
                            {activeDefault && (
                              <span
                                className="providers-badge"
                                title={t("providers.active")}
                              >
                                {t("providers.defaultBadge")}
                              </span>
                            )}
                          </span>
                          <span
                            className="providers-list-model"
                            title={p.model}
                          >
                            {p.model}
                          </span>
                        </span>
                        <span
                          className={`providers-status-dot ${statusDotClass(p.enabled, healthById[p.id])}`}
                        />
                      </div>
                    </div>
                  );
                })(),
                document.body,
              )}

            <div className="providers-add-row">
              <div className="providers-kind-picker">
                <SelectMenu
                  className="providers-kind-select"
                  value={addKind}
                  aria-label={t("providers.add")}
                  openDirection="up"
                  onChange={(v) => setAddKind(v as ProviderKindId)}
                  options={addKindOptions}
                />
              </div>
              <IconButton
                variant="primary"
                className="providers-pane-action providers-add-btn"
                title={t("providers.add")}
                aria-label={t("providers.add")}
                onClick={() => void addProvider()}
              >
                <IconPlus />
              </IconButton>
            </div>
          </Surface>

          <section className="providers-pane providers-pane-detail">
            <div className="providers-pane-head">
              <div className="providers-pane-head-text">
                <h2>{t("providers.detailTitle")}</h2>
              </div>
              {selected && draft && (
                <div className="providers-pane-head-actions">
                  <button
                    type="button"
                    role="switch"
                    className="providers-enabled-control"
                    aria-checked={draft.enabled}
                    aria-label={t("providers.enable")}
                    title={
                      draft.enabled
                        ? t("providers.disable")
                        : t("providers.enable")
                    }
                    disabled={saving}
                    onClick={() => void toggleEnabled()}
                  >
                    <span className="providers-enabled-control-label">
                      {draft.enabled
                        ? t("providers.enabled")
                        : t("providers.disabled")}
                    </span>
                    <span className="providers-switch-track" aria-hidden>
                      <span className="providers-switch-thumb" />
                    </span>
                  </button>
                  <IconButton
                    ref={providerActionsAnchorRef}
                    variant="ghost"
                    className={`providers-pane-action providers-more-action ${providerActionsOpen ? "is-active" : ""}`}
                    title={t("providers.moreActions")}
                    aria-label={t("providers.moreActions")}
                    aria-haspopup="menu"
                    aria-expanded={providerActionsOpen}
                    onClick={() => setProviderActionsOpen((open) => !open)}
                  >
                    <MoreHorizontal size={18} strokeWidth={2} aria-hidden />
                  </IconButton>
                  <PopoverSurface
                    open={providerActionsOpen}
                    onClose={() => setProviderActionsOpen(false)}
                    anchorRef={providerActionsAnchorRef}
                    placement="below"
                    align="end"
                    minWidth={190}
                    className="providers-actions-menu"
                    role="menu"
                    aria-label={t("providers.moreActions")}
                    onKeyDown={(event) => {
                      if (
                        !["ArrowDown", "ArrowUp", "Home", "End"].includes(
                          event.key,
                        )
                      ) {
                        return;
                      }
                      const items = Array.from(
                        event.currentTarget.querySelectorAll<HTMLButtonElement>(
                          '[role="menuitem"]:not(:disabled)',
                        ),
                      );
                      if (items.length === 0) return;
                      event.preventDefault();
                      const current = items.indexOf(
                        document.activeElement as HTMLButtonElement,
                      );
                      const next =
                        event.key === "Home"
                          ? 0
                          : event.key === "End"
                            ? items.length - 1
                            : event.key === "ArrowDown"
                              ? (current + 1 + items.length) % items.length
                              : (current - 1 + items.length) % items.length;
                      items[next]?.focus();
                    }}
                  >
                    {!isActive &&
                    draft.enabled &&
                    selected.supports_responses_api ? (
                      <button
                        type="button"
                        role="menuitem"
                        className="providers-actions-menu-item"
                        onClick={() => {
                          setProviderActionsOpen(false);
                          void setActive(selected.id);
                        }}
                      >
                        <IconStar />
                        <span>{t("providers.setActive")}</span>
                      </button>
                    ) : null}
                    {!isImageActive &&
                    draft.enabled &&
                    selected.supports_image ? (
                      <button
                        type="button"
                        role="menuitem"
                        className="providers-actions-menu-item"
                        onClick={() => {
                          setProviderActionsOpen(false);
                          void setImageActive(selected.id);
                        }}
                      >
                        <Image size={16} strokeWidth={2} aria-hidden />
                        <span>{t("providers.setImageActive")}</span>
                      </button>
                    ) : null}
                    <button
                      type="button"
                      role="menuitem"
                      className="providers-actions-menu-item is-danger"
                      onClick={() => {
                        setProviderActionsOpen(false);
                        void deleteProvider();
                      }}
                    >
                      <IconTrash />
                      <span>{t("providers.delete")}</span>
                    </button>
                  </PopoverSurface>
                </div>
              )}
            </div>

            {!selected || !draft ? (
              <EmptyIllustration
                scene="providers"
                className="providers-empty-illust"
                title={t("providers.emptySelect")}
              />
            ) : (
              <div
                className={`providers-form ${detailTab === "models" ? "is-fill" : ""}`.trim()}
              >
                <div className="providers-form-head">
                  <span className="providers-form-icon" aria-hidden>
                    <ProviderBrandIcon kind={selected.kind} />
                  </span>
                  <div className="providers-form-head-text">
                    <h3>
                      {selected.display_name}
                      {isActive && (
                        <span className="providers-badge">
                          {t("providers.active")}
                        </span>
                      )}
                      {isImageActive && (
                        <span className="providers-badge">
                          {t("providers.imageActive")}
                        </span>
                      )}
                    </h3>
                    <p className="providers-form-kind">
                      {t(kindLabelKey(selected.kind))}
                    </p>
                  </div>
                </div>

                <div className="providers-detail-tabs" role="tablist">
                  <button
                    type="button"
                    role="tab"
                    aria-selected={detailTab === "chat"}
                    className={`providers-detail-tab ${detailTab === "chat" ? "is-active" : ""}`}
                    onClick={() => setDetailTab("chat")}
                  >
                    <MessageCircle size={14} strokeWidth={2} aria-hidden />
                    {t("providers.tabChat")}
                  </button>
                  <button
                    type="button"
                    role="tab"
                    aria-selected={detailTab === "models"}
                    className={`providers-detail-tab ${detailTab === "models" ? "is-active" : ""}`}
                    onClick={() => setDetailTab("models")}
                  >
                    <Layers size={14} strokeWidth={2} aria-hidden />
                    {t("providers.tabModels")}
                    {models.length > 0 ? (
                      <span className="providers-detail-tab-count">
                        {models.length}
                      </span>
                    ) : null}
                  </button>
                  {supportsMediaModels(selected) ? (
                    <button
                      type="button"
                      role="tab"
                      aria-selected={detailTab === "media"}
                      className={`providers-detail-tab ${detailTab === "media" ? "is-active" : ""}`}
                      onClick={() => setDetailTab("media")}
                    >
                      <Image size={14} strokeWidth={2} aria-hidden />
                      {t("providers.tabMedia")}
                    </button>
                  ) : null}
                  {supportsVoice(selected) ? (
                    <button
                      type="button"
                      role="tab"
                      aria-selected={detailTab === "voice"}
                      className={`providers-detail-tab ${detailTab === "voice" ? "is-active" : ""}`}
                      onClick={() => setDetailTab("voice")}
                    >
                      <Mic size={14} strokeWidth={2} aria-hidden />
                      {t("providers.tabVoice")}
                    </button>
                  ) : null}
                  {supportsEmbedding(selected) ? (
                    <button
                      type="button"
                      role="tab"
                      aria-selected={detailTab === "embedding"}
                      className={`providers-detail-tab ${detailTab === "embedding" ? "is-active" : ""}`}
                      onClick={() => setDetailTab("embedding")}
                    >
                      <Layers size={14} strokeWidth={2} aria-hidden />
                      {t("providers.tabEmbedding")}
                    </button>
                  ) : null}
                </div>

                {detailTab === "chat" && (
                  <>
                    <div className="providers-form-grid">
                      <label className="providers-field">
                        <span className="providers-field-label">
                          <IconTag />
                          {t("providers.displayName")}
                        </span>
                        <input
                          type="text"
                          value={draft.display_name}
                          onChange={(e) =>
                            setDraft((d) =>
                              d ? { ...d, display_name: e.target.value } : d,
                            )
                          }
                        />
                      </label>

                      <label className="providers-field">
                        <span className="providers-field-label">
                          <IconBox />
                          {t("providers.model")}
                        </span>
                        <input
                          type="text"
                          value={draft.model}
                          onChange={(e) =>
                            setDraft((d) =>
                              d ? { ...d, model: e.target.value } : d,
                            )
                          }
                        />
                      </label>
                      {selected.kind === "volcengine" && (
                        <p className="providers-field-hint">
                          {t("providers.volcengineModelHint")}
                        </p>
                      )}

                      <label className="providers-field providers-field-span">
                        <span className="providers-field-label">
                          <IconLink />
                          {t("providers.endpoint")}
                          {selected.kind === "google" &&
                            /\/v1beta\/openai(?:\/|$)/.test(
                              draft.endpoint.trim(),
                            ) && (
                              <span
                                className="providers-badge providers-badge--deprecated"
                                title={t(
                                  "providers.googleOpenaiCompatDeprecated",
                                )}
                              >
                                {t("providers.googleOpenaiCompatDeprecated")}
                              </span>
                            )}
                        </span>
                        <input
                          type="url"
                          value={draft.endpoint}
                          onChange={(e) =>
                            setDraft((d) =>
                              d ? { ...d, endpoint: e.target.value } : d,
                            )
                          }
                        />
                      </label>
                      {draft.endpoint.trim() && (
                        <p className="providers-field-hint">
                          →{" "}
                          {resolveEndpointPreview(
                            selected.kind,
                            draft.endpoint,
                            selected.supports_responses_api
                              ? "responses"
                              : undefined,
                          )}
                        </p>
                      )}
                    </div>

                    <div className="providers-fallback-block">
                      <div className="providers-fallback-head">
                        <h4 className="providers-block-title">
                          <IconWaypoints />
                          {t("providers.fallback.title")}
                        </h4>
                        <span className="providers-fallback-count">
                          {fallbackEntries.length}/{MAX_MODEL_FALLBACKS}
                        </span>
                      </div>
                      <p className="providers-fallback-hint">
                        {fallbackEntries.length === 0
                          ? t("providers.fallback.hintWithLimit", {
                              max: String(MAX_MODEL_FALLBACKS),
                            })
                          : t("providers.fallback.hint")}
                      </p>
                      {fallbackEntries.length > 0 && (
                        <ul className="providers-fallback-list">
                          {fallbackEntries.map((entry, index) => {
                            const options =
                              fallbackModelsById[entry.provider_id] ?? [];
                            const fallbackProvider = state?.providers.find(
                              (p) => p.id === entry.provider_id,
                            );
                            const defaultModel =
                              fallbackProvider?.model?.trim() || "";
                            const selectedModel = entry.model?.trim() || "";
                            const knownIds = new Set(options.map((m) => m.id));
                            const orphanSelected =
                              selectedModel && !knownIds.has(selectedModel)
                                ? selectedModel
                                : null;
                            return (
                              <li
                                key={`${entry.provider_id}-${index}`}
                                className="providers-fallback-row"
                              >
                                <span
                                  className="providers-fallback-brand"
                                  aria-hidden
                                >
                                  {fallbackProvider ? (
                                    <ProviderBrandIcon
                                      kind={fallbackProvider.kind}
                                    />
                                  ) : (
                                    <IconLayers />
                                  )}
                                </span>
                                <span
                                  className="providers-fallback-name"
                                  title={entry.provider_id}
                                >
                                  {providerLabel(entry.provider_id)}
                                </span>
                                <SelectMenu
                                  className="providers-fallback-model"
                                  value={selectedModel}
                                  placeholder={t(
                                    "providers.fallback.defaultModel",
                                  )}
                                  aria-label={t(
                                    "providers.fallback.modelOverride",
                                    { name: providerLabel(entry.provider_id) },
                                  )}
                                  onChange={(v) =>
                                    updateFallbackModel(index, v)
                                  }
                                  options={[
                                    {
                                      value: "",
                                      label: defaultModel
                                        ? t(
                                            "providers.fallback.defaultModelValue",
                                            { model: defaultModel },
                                          )
                                        : t("providers.fallback.defaultModel"),
                                    },
                                    ...(orphanSelected
                                      ? [
                                          {
                                            value: orphanSelected,
                                            label: orphanSelected,
                                            icon: (
                                              <ModelBrandIcon
                                                modelId={orphanSelected}
                                              />
                                            ),
                                          },
                                        ]
                                      : []),
                                    ...options.map((m) => ({
                                      value: m.id,
                                      label: m.display_name?.trim()
                                        ? `${m.display_name} (${m.id})`
                                        : m.id,
                                      icon: <ModelBrandIcon modelId={m.id} />,
                                    })),
                                  ]}
                                />
                                <button
                                  type="button"
                                  className="providers-icon-btn"
                                  title={t("providers.fallback.removeTitle")}
                                  aria-label={t(
                                    "providers.fallback.removeAriaLabel",
                                    { name: providerLabel(entry.provider_id) },
                                  )}
                                  onClick={() => removeFallback(index)}
                                >
                                  <IconTrash />
                                </button>
                              </li>
                            );
                          })}
                        </ul>
                      )}
                      {fallbackEntries.length < MAX_MODEL_FALLBACKS && (
                        <div className="providers-fallback-add">
                          <span
                            className="providers-fallback-add-icon"
                            aria-hidden
                          >
                            <IconPlus />
                          </span>
                          <SelectMenu
                            className="providers-fallback-add-select"
                            value=""
                            aria-label={t("providers.fallback.addAriaLabel")}
                            disabled={fallbackCandidateProviders.length === 0}
                            placeholder={
                              fallbackCandidateProviders.length === 0
                                ? t("providers.fallback.noneAvailable")
                                : t("providers.fallback.addPlaceholder")
                            }
                            onChange={addFallback}
                            options={fallbackCandidateProviders.map((p) => ({
                              value: p.id,
                              label: p.display_name,
                              icon: <ProviderBrandIcon kind={p.kind} />,
                            }))}
                          />
                        </div>
                      )}
                    </div>

                    <div className="providers-key-block">
                      <div className="providers-key-head">
                        <h4 className="providers-block-title">
                          <IconKey />
                          {t("providers.apiKey")}
                        </h4>
                        {needsKey && selected.official_key_url && (
                          <Button
                            variant="ghost"
                            size="sm"
                            className="providers-key-link"
                            onClick={() => void openOfficialKey()}
                          >
                            {t("providers.getOfficialKey")}
                          </Button>
                        )}
                      </div>
                      {!needsKey ? (
                        <p className="providers-key-hint">
                          {t("providers.noKeyRequired")}
                        </p>
                      ) : (
                        <>
                          <div className="providers-key-row">
                            <div className="providers-key-input-wrap">
                              <input
                                type="text"
                                className="providers-key-input"
                                value={apiKeyDisplayValue}
                                readOnly={!apiKeyDirty && !!storedApiKey}
                                onChange={(e) => {
                                  setApiKeyDirty(true);
                                  setApiKeyInput(e.target.value);
                                }}
                                onFocus={() => {
                                  if (!apiKeyDirty && storedApiKey) {
                                    setApiKeyDirty(true);
                                    setApiKeyInput(storedApiKey);
                                  }
                                }}
                                onBlur={() => {
                                  if (
                                    apiKeyDirty &&
                                    storedApiKey &&
                                    apiKeyInput.trim() === storedApiKey.trim()
                                  ) {
                                    setApiKeyDirty(false);
                                    setApiKeyInput("");
                                  }
                                }}
                                placeholder={t("providers.apiKeyPlaceholder")}
                                autoComplete="off"
                                spellCheck={false}
                              />
                              {storedApiKey && !apiKeyDirty && (
                                <button
                                  type="button"
                                  className="providers-key-eye"
                                  title={
                                    showApiKey
                                      ? t("providers.hideKey")
                                      : t("providers.showKey")
                                  }
                                  aria-label={
                                    showApiKey
                                      ? t("providers.hideKey")
                                      : t("providers.showKey")
                                  }
                                  onClick={() => setShowApiKey((v) => !v)}
                                >
                                  <IconKeyVisibility visible={showApiKey} />
                                </button>
                              )}
                            </div>
                            <button
                              type="button"
                              className="providers-icon-btn is-primary"
                              disabled={!canSaveApiKey || saving}
                              title={t("providers.saveKey")}
                              aria-label={t("providers.saveKey")}
                              onClick={() => void saveApiKey()}
                            >
                              {saving ? (
                                <IconLoader className="is-spin" />
                              ) : (
                                <IconSave />
                              )}
                            </button>
                            <button
                              type="button"
                              className={`providers-icon-btn ${testing ? "is-busy" : ""}`}
                              disabled={testing || !selected.has_api_key}
                              title={
                                testing
                                  ? t("providers.testing")
                                  : t("providers.test")
                              }
                              aria-label={
                                testing
                                  ? t("providers.testing")
                                  : t("providers.test")
                              }
                              aria-busy={testing}
                              onClick={() => void testConnection()}
                            >
                              {testing ? (
                                <IconLoader className="is-spin" />
                              ) : (
                                <IconStethoscope />
                              )}
                            </button>
                            {selected.key_source === "keyring" && (
                              <button
                                type="button"
                                className="providers-icon-btn"
                                title={t("providers.clearKey")}
                                aria-label={t("providers.clearKey")}
                                onClick={() => void clearApiKey()}
                              >
                                <IconTrash />
                              </button>
                            )}
                          </div>
                          {testResult && (
                            <p
                              className={`providers-test-result ${testResult.ok ? "ok" : "fail"}`}
                            >
                              {testResult.ok
                                ? t("providers.testOk", {
                                    ms: String(testResult.latency_ms),
                                  })
                                : t("providers.testFail", {
                                    ms: String(testResult.latency_ms),
                                  })}
                              <span className="providers-test-model">
                                {" "}
                                · {testResult.model}
                              </span>
                            </p>
                          )}
                        </>
                      )}
                    </div>
                  </>
                )}

                {detailTab === "models" && (
                  <div className="providers-models-panel">
                    <div className="providers-models-block">
                      <div className="providers-models-head">
                        <div className="providers-models-title-row">
                          <h4 className="providers-block-title">
                            <IconLayers />
                            {t("providers.modelsTitle")}
                          </h4>
                          {models.length > 0 && (
                            <span className="providers-models-count">
                              {filteredModels.length}
                            </span>
                          )}
                          {modelsLatency != null && (
                            <span
                              className="providers-models-latency"
                              title={t("providers.modelsLatency", {
                                ms: String(modelsLatency),
                              })}
                            >
                              {modelsLatency} ms
                            </span>
                          )}
                        </div>
                        <div className="providers-models-toolbar">
                          <button
                            type="button"
                            className={`providers-icon-btn ${testingAll ? "is-busy" : ""}`}
                            disabled={
                              testingAll ||
                              testing ||
                              models.length === 0 ||
                              (needsKey && !selected.has_api_key)
                            }
                            title={t("providers.healthCheck")}
                            aria-label={t("providers.healthCheck")}
                            aria-busy={testingAll}
                            onClick={() => void testAllModels()}
                          >
                            <IconStethoscope />
                          </button>
                          <button
                            type="button"
                            className={`providers-icon-btn ${showFilter ? "is-active" : ""}`}
                            disabled={models.length === 0}
                            title={t("providers.toggleSearch")}
                            aria-label={t("providers.toggleSearch")}
                            aria-pressed={showFilter}
                            onClick={() => setShowFilter((v) => !v)}
                          >
                            <IconSearch />
                          </button>
                          <button
                            type="button"
                            className={`providers-icon-btn ${sortByNewest ? "is-active" : ""}`}
                            disabled={models.length === 0}
                            title={t("providers.sortNewest")}
                            aria-label={t("providers.sortNewest")}
                            aria-pressed={sortByNewest}
                            onClick={() => setSortByNewest((v) => !v)}
                          >
                            <IconSortNewest />
                          </button>
                          <button
                            type="button"
                            className={`providers-icon-btn ${onlyThisWeek ? "is-active" : ""}`}
                            disabled={models.length === 0}
                            title={t("providers.filterThisWeek")}
                            aria-label={t("providers.filterThisWeek")}
                            aria-pressed={onlyThisWeek}
                            onClick={() => setOnlyThisWeek((v) => !v)}
                          >
                            <IconThisWeek />
                          </button>
                          {!needsKey && (
                            <button
                              type="button"
                              className={`providers-icon-btn ${testing ? "is-busy" : ""}`}
                              disabled={testing || testingAll}
                              title={
                                testing
                                  ? t("providers.testing")
                                  : t("providers.test")
                              }
                              aria-label={
                                testing
                                  ? t("providers.testing")
                                  : t("providers.test")
                              }
                              aria-busy={testing}
                              onClick={() => void testConnection()}
                            >
                              {testing ? (
                                <IconLoader className="is-spin" />
                              ) : (
                                <IconStethoscope />
                              )}
                            </button>
                          )}
                          <button
                            type="button"
                            className={`providers-icon-btn is-primary ${listingModels ? "is-busy" : ""}`}
                            disabled={
                              listingModels ||
                              (needsKey && !selected.has_api_key)
                            }
                            title={
                              listingModels
                                ? t("providers.listingModels")
                                : t("providers.refreshModels")
                            }
                            aria-label={
                              listingModels
                                ? t("providers.listingModels")
                                : t("providers.refreshModels")
                            }
                            aria-busy={listingModels}
                            onClick={() => void listModels()}
                          >
                            {listingModels ? (
                              <IconLoader className="is-spin" />
                            ) : (
                              <IconRefresh />
                            )}
                          </button>
                          <button
                            type="button"
                            className={`providers-icon-btn ${showAddModel ? "is-active" : ""}`}
                            title={t("providers.addModel")}
                            aria-label={t("providers.addModel")}
                            aria-pressed={showAddModel}
                            onClick={() => {
                              setShowAddModel((v) => !v);
                              setShowFilter(false);
                            }}
                          >
                            <IconPlus />
                          </button>
                        </div>
                      </div>

                      {(showFilter || modelFilter) && (
                        <input
                          type="search"
                          className="providers-model-filter"
                          value={modelFilter}
                          onChange={(e) => setModelFilter(e.target.value)}
                          placeholder={t("providers.modelFilter")}
                          disabled={models.length === 0}
                          autoFocus={showFilter}
                        />
                      )}

                      {showAddModel && (
                        <form
                          className="providers-add-model-row"
                          onSubmit={(e) => {
                            e.preventDefault();
                            addCustomModel();
                          }}
                        >
                          <input
                            type="text"
                            className="providers-model-filter"
                            value={customModelInput}
                            onChange={(e) =>
                              setCustomModelInput(e.target.value)
                            }
                            placeholder={t("providers.addModelPlaceholder")}
                            autoFocus
                            spellCheck={false}
                          />
                          <button
                            type="submit"
                            className="providers-icon-btn is-primary"
                            disabled={!customModelInput.trim()}
                            title={t("providers.addModelConfirm")}
                            aria-label={t("providers.addModelConfirm")}
                          >
                            <IconPlus />
                          </button>
                        </form>
                      )}

                      {listingModels && models.length === 0 ? (
                        <div className="providers-models-empty">
                          <p>{t("providers.listingModels")}</p>
                        </div>
                      ) : models.length === 0 ? (
                        <div className="providers-models-empty">
                          <p>{t("providers.modelsEmpty")}</p>
                        </div>
                      ) : filteredModels.length === 0 ? (
                        <div className="providers-models-empty">
                          <p>{t("providers.modelsNoMatch")}</p>
                        </div>
                      ) : (
                        <ul className="providers-models-list">
                          {filteredModels.map((m) => {
                            const isCurrent = draft.model === m.id;
                            const latency = modelLatencies[m.id];
                            const caps = m.capabilities;
                            const ctxLabel = formatContextWindow(
                              m.context_window,
                            );
                            const priceLabel = formatModelPrice(m.pricing);
                            const cutoffLabel = formatKnowledgeCutoff(
                              m.knowledge_cutoff,
                            );
                            const expired = Boolean(m.expiration_date?.trim());
                            const createdLabel = formatModelCreated(m.created);
                            const isNewThisWeek = isModelCreatedWithinDays(
                              m.created,
                              7,
                            );
                            const hfId = m.hugging_face_id?.trim() || null;
                            const moderated = m.is_moderated === true;
                            const expanded = expandedModelId === m.id;
                            const displayName = m.display_name?.trim() || null;
                            const activeCaps = listActiveModelCaps(caps);
                            const hasDetail =
                              Boolean(m.description?.trim()) ||
                              Boolean(createdLabel) ||
                              Boolean(cutoffLabel) ||
                              Boolean(m.expiration_date?.trim()) ||
                              Boolean(ctxLabel) ||
                              Boolean(priceLabel) ||
                              Boolean(hfId) ||
                              moderated ||
                              Boolean(latency) ||
                              activeCaps.length > 0;
                            const capIcons = (
                              <span
                                className="providers-model-caps"
                                aria-label="capabilities"
                              >
                                {caps.vision && (
                                  <span
                                    className="providers-cap vision"
                                    title={t("providers.cap.vision")}
                                  >
                                    <Eye
                                      size={14}
                                      strokeWidth={2}
                                      aria-hidden
                                    />
                                  </span>
                                )}
                                {caps.file && (
                                  <span
                                    className="providers-cap file"
                                    title={t("providers.cap.file")}
                                  >
                                    <FileText
                                      size={14}
                                      strokeWidth={2}
                                      aria-hidden
                                    />
                                  </span>
                                )}
                                {caps.audio_in && (
                                  <span
                                    className="providers-cap audio-in"
                                    title={t("providers.cap.audioIn")}
                                  >
                                    <Mic
                                      size={14}
                                      strokeWidth={2}
                                      aria-hidden
                                    />
                                  </span>
                                )}
                                {caps.web && (
                                  <span
                                    className="providers-cap web"
                                    title={t("providers.cap.web")}
                                  >
                                    <Globe
                                      size={14}
                                      strokeWidth={2}
                                      aria-hidden
                                    />
                                  </span>
                                )}
                                {caps.reasoning && (
                                  <span
                                    className="providers-cap reasoning"
                                    title={t("providers.cap.reasoning")}
                                  >
                                    <Lightbulb
                                      size={14}
                                      strokeWidth={2}
                                      aria-hidden
                                    />
                                  </span>
                                )}
                                {caps.tools && (
                                  <span
                                    className="providers-cap tools"
                                    title={t("providers.cap.tools")}
                                  >
                                    <Wrench
                                      size={14}
                                      strokeWidth={2}
                                      aria-hidden
                                    />
                                  </span>
                                )}
                                {caps.image_gen && (
                                  <span
                                    className="providers-cap image-gen"
                                    title={t("providers.cap.imageGen")}
                                  >
                                    <Image
                                      size={14}
                                      strokeWidth={2}
                                      aria-hidden
                                    />
                                  </span>
                                )}
                                {caps.video_gen && (
                                  <span
                                    className="providers-cap video-gen"
                                    title={t("providers.cap.videoGen")}
                                  >
                                    <Video
                                      size={14}
                                      strokeWidth={2}
                                      aria-hidden
                                    />
                                  </span>
                                )}
                                {caps.audio_gen && (
                                  <span
                                    className="providers-cap audio-gen"
                                    title={t("providers.cap.audioGen")}
                                  >
                                    <Mic
                                      size={14}
                                      strokeWidth={2}
                                      aria-hidden
                                    />
                                  </span>
                                )}
                                {caps.music_gen && (
                                  <span
                                    className="providers-cap music-gen"
                                    title={t("providers.cap.musicGen")}
                                  >
                                    <Music
                                      size={14}
                                      strokeWidth={2}
                                      aria-hidden
                                    />
                                  </span>
                                )}
                              </span>
                            );
                            return (
                              <li
                                key={m.id}
                                className={`providers-model-row ${isCurrent ? "is-current" : ""} ${expired ? "is-expiring" : ""} ${expanded ? "is-expanded" : ""}`}
                              >
                                <div className="providers-model-main">
                                  <label className="providers-model-pick">
                                    <input
                                      type="radio"
                                      className="providers-model-radio"
                                      name="providers-model-select"
                                      checked={isCurrent}
                                      onChange={() =>
                                        void useModel(m.id, m.expiration_date)
                                      }
                                      title={t("providers.useModel")}
                                      aria-label={`${t("providers.useModel")}: ${m.id}`}
                                    />
                                    <ModelBrandIcon
                                      modelId={m.id}
                                      className="providers-model-icon"
                                    />
                                    <span className="providers-model-text">
                                      <span
                                        className="providers-model-id"
                                        title={displayName ?? m.id}
                                      >
                                        {m.id}
                                      </span>
                                      {displayName && displayName !== m.id ? (
                                        <span
                                          className="providers-model-display"
                                          title={displayName}
                                        >
                                          {displayName}
                                        </span>
                                      ) : null}
                                    </span>
                                    {expired && (
                                      <span
                                        className="providers-model-badge is-expiring"
                                        title={`${t("providers.expiration")}: ${m.expiration_date}`}
                                      >
                                        {t("providers.expiring")}
                                      </span>
                                    )}
                                    {isNewThisWeek && (
                                      <span
                                        className="providers-model-badge is-new"
                                        title={
                                          createdLabel
                                            ? `${t("providers.created")}: ${createdLabel}`
                                            : t("providers.newThisWeek")
                                        }
                                      >
                                        {t("providers.newThisWeek")}
                                      </span>
                                    )}
                                  </label>
                                  {capIcons}
                                  <span className="providers-model-actions">
                                    {hasDetail ? (
                                      <button
                                        type="button"
                                        className={`providers-icon-btn ${expanded ? "is-active" : ""}`}
                                        title={
                                          expanded
                                            ? t("providers.hideModelDetails")
                                            : t("providers.showModelDetails")
                                        }
                                        aria-label={
                                          expanded
                                            ? t("providers.hideModelDetails")
                                            : t("providers.showModelDetails")
                                        }
                                        aria-expanded={expanded}
                                        onClick={(e) => {
                                          e.preventDefault();
                                          e.stopPropagation();
                                          setExpandedModelId((id) =>
                                            id === m.id ? null : m.id,
                                          );
                                        }}
                                      >
                                        <IconInfo />
                                      </button>
                                    ) : null}
                                    <button
                                      type="button"
                                      className={`providers-icon-btn ${testing && isCurrent ? "is-busy" : ""}`}
                                      disabled={testing || testingAll}
                                      title={t("providers.test")}
                                      aria-label={`${t("providers.test")}: ${m.id}`}
                                      onClick={() => {
                                        void (async () => {
                                          const ok = await useModel(
                                            m.id,
                                            m.expiration_date,
                                          );
                                          if (!ok) return;
                                          void testConnection(m.id);
                                        })();
                                      }}
                                    >
                                      <IconStethoscope />
                                    </button>
                                  </span>
                                </div>
                                {expanded ? (
                                  <div className="providers-model-detail">
                                    {m.description?.trim() ? (
                                      <p className="providers-model-desc">
                                        {m.description.trim()}
                                      </p>
                                    ) : null}

                                    <div className="providers-model-meta-grid">
                                      {createdLabel ? (
                                        <div className="providers-meta-chip">
                                          <span
                                            className="providers-meta-chip-icon"
                                            aria-hidden
                                          >
                                            <Calendar
                                              size={14}
                                              strokeWidth={2}
                                            />
                                          </span>
                                          <span className="providers-meta-chip-body">
                                            <span className="providers-meta-chip-label">
                                              {t("providers.created")}
                                            </span>
                                            <span className="providers-meta-chip-value">
                                              {createdLabel}
                                            </span>
                                          </span>
                                        </div>
                                      ) : null}
                                      {cutoffLabel ? (
                                        <div
                                          className="providers-meta-chip"
                                          title={
                                            m.knowledge_cutoff ?? undefined
                                          }
                                        >
                                          <span
                                            className="providers-meta-chip-icon"
                                            aria-hidden
                                          >
                                            <BookOpen
                                              size={14}
                                              strokeWidth={2}
                                            />
                                          </span>
                                          <span className="providers-meta-chip-body">
                                            <span className="providers-meta-chip-label">
                                              {t("providers.knowledgeCutoff")}
                                            </span>
                                            <span className="providers-meta-chip-value">
                                              {cutoffLabel}
                                            </span>
                                          </span>
                                        </div>
                                      ) : null}
                                      {m.expiration_date?.trim() ? (
                                        <div className="providers-meta-chip is-warn">
                                          <span
                                            className="providers-meta-chip-icon"
                                            aria-hidden
                                          >
                                            <Timer size={14} strokeWidth={2} />
                                          </span>
                                          <span className="providers-meta-chip-body">
                                            <span className="providers-meta-chip-label">
                                              {t("providers.expiration")}
                                            </span>
                                            <span className="providers-meta-chip-value">
                                              {m.expiration_date}
                                            </span>
                                          </span>
                                        </div>
                                      ) : null}
                                      {ctxLabel ? (
                                        <div className="providers-meta-chip">
                                          <span
                                            className="providers-meta-chip-icon"
                                            aria-hidden
                                          >
                                            <Layers size={14} strokeWidth={2} />
                                          </span>
                                          <span className="providers-meta-chip-body">
                                            <span className="providers-meta-chip-label">
                                              {t("providers.contextWindow")}
                                            </span>
                                            <span className="providers-meta-chip-value">
                                              {ctxLabel}
                                            </span>
                                          </span>
                                        </div>
                                      ) : null}
                                      {priceLabel ? (
                                        <div className="providers-meta-chip">
                                          <span
                                            className="providers-meta-chip-icon"
                                            aria-hidden
                                          >
                                            <CircleDollarSign
                                              size={14}
                                              strokeWidth={2}
                                            />
                                          </span>
                                          <span className="providers-meta-chip-body">
                                            <span className="providers-meta-chip-label">
                                              {t("providers.pricePerM")}
                                            </span>
                                            <span className="providers-meta-chip-value">
                                              {priceLabel}
                                            </span>
                                          </span>
                                        </div>
                                      ) : null}
                                      {moderated ? (
                                        <div className="providers-meta-chip">
                                          <span
                                            className="providers-meta-chip-icon"
                                            aria-hidden
                                          >
                                            <ShieldCheck
                                              size={14}
                                              strokeWidth={2}
                                            />
                                          </span>
                                          <span className="providers-meta-chip-body">
                                            <span className="providers-meta-chip-label">
                                              {t("providers.moderated")}
                                            </span>
                                            <span className="providers-meta-chip-value">
                                              {t("providers.yes")}
                                            </span>
                                          </span>
                                        </div>
                                      ) : null}
                                      {hfId ? (
                                        <a
                                          className="providers-meta-chip is-link"
                                          href={`https://huggingface.co/${hfId}`}
                                          target="_blank"
                                          rel="noreferrer"
                                          onClick={(e) => e.stopPropagation()}
                                          title={`${t("providers.huggingFace")}: ${hfId}`}
                                        >
                                          <span
                                            className="providers-meta-chip-icon"
                                            aria-hidden
                                          >
                                            <ExternalLink
                                              size={14}
                                              strokeWidth={2}
                                            />
                                          </span>
                                          <span className="providers-meta-chip-body">
                                            <span className="providers-meta-chip-label">
                                              {t("providers.huggingFace")}
                                            </span>
                                            <span className="providers-meta-chip-value">
                                              {hfId}
                                            </span>
                                          </span>
                                        </a>
                                      ) : null}
                                      {latency ? (
                                        <div
                                          className={`providers-meta-chip ${latency.ok ? "is-ok" : "is-fail"}`}
                                        >
                                          <span
                                            className="providers-meta-chip-icon"
                                            aria-hidden
                                          >
                                            <Stethoscope
                                              size={14}
                                              strokeWidth={2}
                                            />
                                          </span>
                                          <span className="providers-meta-chip-body">
                                            <span className="providers-meta-chip-label">
                                              {t("providers.test")}
                                            </span>
                                            <span className="providers-meta-chip-value">
                                              {latency.latency_ms < 0
                                                ? "—"
                                                : `${latency.latency_ms} ms`}
                                            </span>
                                          </span>
                                        </div>
                                      ) : null}
                                    </div>

                                    {activeCaps.length > 0 ? (
                                      <div className="providers-model-cap-section">
                                        <div className="providers-model-cap-heading">
                                          {t("providers.capabilities")}
                                        </div>
                                        <div className="providers-model-cap-pills">
                                          {caps.tools ? (
                                            <span className="providers-cap-pill tools">
                                              <Wrench
                                                size={13}
                                                strokeWidth={2}
                                                aria-hidden
                                              />
                                              {t("providers.cap.tools")}
                                            </span>
                                          ) : null}
                                          {caps.reasoning ? (
                                            <span className="providers-cap-pill reasoning">
                                              <Lightbulb
                                                size={13}
                                                strokeWidth={2}
                                                aria-hidden
                                              />
                                              {t("providers.cap.reasoning")}
                                            </span>
                                          ) : null}
                                          {caps.vision ? (
                                            <span className="providers-cap-pill vision">
                                              <Eye
                                                size={13}
                                                strokeWidth={2}
                                                aria-hidden
                                              />
                                              {t("providers.cap.vision")}
                                            </span>
                                          ) : null}
                                          {caps.file ? (
                                            <span className="providers-cap-pill file">
                                              <FileText
                                                size={13}
                                                strokeWidth={2}
                                                aria-hidden
                                              />
                                              {t("providers.cap.file")}
                                            </span>
                                          ) : null}
                                          {caps.audio_in ? (
                                            <span className="providers-cap-pill audio-in">
                                              <Mic
                                                size={13}
                                                strokeWidth={2}
                                                aria-hidden
                                              />
                                              {t("providers.cap.audioIn")}
                                            </span>
                                          ) : null}
                                          {caps.web ? (
                                            <span className="providers-cap-pill web">
                                              <Globe
                                                size={13}
                                                strokeWidth={2}
                                                aria-hidden
                                              />
                                              {t("providers.cap.web")}
                                            </span>
                                          ) : null}
                                          {caps.image_gen ? (
                                            <span className="providers-cap-pill image-gen">
                                              <Image
                                                size={13}
                                                strokeWidth={2}
                                                aria-hidden
                                              />
                                              {t("providers.cap.imageGen")}
                                            </span>
                                          ) : null}
                                          {caps.video_gen ? (
                                            <span className="providers-cap-pill video-gen">
                                              <Video
                                                size={13}
                                                strokeWidth={2}
                                                aria-hidden
                                              />
                                              {t("providers.cap.videoGen")}
                                            </span>
                                          ) : null}
                                          {caps.audio_gen ? (
                                            <span className="providers-cap-pill audio-gen">
                                              <Mic
                                                size={13}
                                                strokeWidth={2}
                                                aria-hidden
                                              />
                                              {t("providers.cap.audioGen")}
                                            </span>
                                          ) : null}
                                          {caps.music_gen ? (
                                            <span className="providers-cap-pill music-gen">
                                              <Music
                                                size={13}
                                                strokeWidth={2}
                                                aria-hidden
                                              />
                                              {t("providers.cap.musicGen")}
                                            </span>
                                          ) : null}
                                        </div>
                                      </div>
                                    ) : null}
                                  </div>
                                ) : null}
                              </li>
                            );
                          })}
                        </ul>
                      )}
                    </div>
                  </div>
                )}

                {detailTab === "media" && supportsMediaModels(selected) && (
                  <div className="providers-media-panel">
                    <p className="providers-field-hint providers-media-hint">
                      {t("providers.mediaHint")}
                    </p>
                    <div className="providers-form-grid">
                      <label className="providers-field providers-field-span">
                        <span className="providers-field-label">
                          <IconBox />
                          {t("providers.imageModel")}
                        </span>
                        <SelectMenu
                          className="providers-media-model-select"
                          value={draft.image_model}
                          aria-label={t("providers.imageModel")}
                          onChange={(value) =>
                            setDraft((current) =>
                              current
                                ? { ...current, image_model: value }
                                : current,
                            )
                          }
                          options={mediaOptions(
                            "image_gen",
                            MEDIA_MODEL_DEFAULTS[selected.kind]?.image ?? "",
                          )}
                        />
                      </label>
                      {selected.supports_video ? (
                        <label className="providers-field providers-field-span">
                          <span className="providers-field-label">
                            <IconBox />
                            {t("providers.videoModel")}
                          </span>
                          <SelectMenu
                            className="providers-media-model-select"
                            value={draft.video_model}
                            aria-label={t("providers.videoModel")}
                            onChange={(value) =>
                              setDraft((current) =>
                                current
                                  ? { ...current, video_model: value }
                                  : current,
                              )
                            }
                            options={mediaOptions(
                              "video_gen",
                              MEDIA_MODEL_DEFAULTS[selected.kind]?.video ?? "",
                            )}
                          />
                        </label>
                      ) : null}
                      {selected.supports_music ? (
                        <label className="providers-field providers-field-span">
                          <span className="providers-field-label">
                            <IconBox />
                            {t("providers.musicModel")}
                          </span>
                          <SelectMenu
                            className="providers-media-model-select"
                            value={draft.music_model}
                            aria-label={t("providers.musicModel")}
                            onChange={(value) =>
                              setDraft((current) =>
                                current
                                  ? { ...current, music_model: value }
                                  : current,
                              )
                            }
                            options={mediaOptions(
                              "music_gen",
                              MEDIA_MODEL_DEFAULTS[selected.kind]?.music ?? "",
                            )}
                          />
                        </label>
                      ) : null}
                      <label className="providers-field providers-field-span">
                        <span className="providers-field-label">
                          <IconBox />
                          {t("providers.visionModel")}
                        </span>
                        <SelectMenu
                          className="providers-media-model-select"
                          value={draft.vision_model}
                          aria-label={t("providers.visionModel")}
                          onChange={(value) =>
                            setDraft((current) =>
                              current
                                ? { ...current, vision_model: value }
                                : current,
                            )
                          }
                          options={mediaOptions(
                            "vision",
                            MEDIA_MODEL_DEFAULTS[selected.kind]?.vision ?? "",
                          )}
                        />
                      </label>
                    </div>
                  </div>
                )}

                {detailTab === "voice" && supportsVoice(selected) && (
                  <div className="providers-media-panel">
                    <div className="providers-form-grid">
                      {selected.supports_tts ? (
                        <label className="providers-field providers-field-span">
                          <span className="providers-field-label">
                            <IconBox />
                            {t("providers.ttsModel")}
                          </span>
                          <SelectMenu
                            className="providers-media-model-select"
                            value={draft.tts_model}
                            aria-label={t("providers.ttsModel")}
                            onChange={(value) =>
                              setDraft((current) =>
                                current
                                  ? { ...current, tts_model: value }
                                  : current,
                              )
                            }
                            options={mediaOptions(
                              "audio_gen",
                              MEDIA_MODEL_DEFAULTS[selected.kind]?.tts ?? "",
                            )}
                          />
                        </label>
                      ) : null}
                      {selected.supports_asr ? (
                        <label className="providers-field providers-field-span">
                          <span className="providers-field-label">
                            <IconBox />
                            {t("providers.asrModel")}
                          </span>
                          <input
                            className="providers-input"
                            value={selected.asr_model ?? ""}
                            readOnly
                            placeholder="—"
                          />
                          <span className="providers-field-hint">
                            {t("providers.asrModelHint")}
                          </span>
                        </label>
                      ) : null}
                    </div>
                  </div>
                )}

                {detailTab === "embedding" && supportsEmbedding(selected) && (
                  <div className="providers-media-panel">
                    <p className="providers-field-hint providers-media-hint">
                      {t("providers.embeddingHint")}
                    </p>
                    <div className="providers-form-grid">
                      <label className="providers-field providers-field-span">
                        <span className="providers-field-label">
                          <IconBox />
                          {t("providers.embeddingModel")}
                        </span>
                        <input
                          className="providers-input"
                          value={draft.embedding_model}
                          onChange={(e) =>
                            setDraft((current) =>
                              current
                                ? {
                                    ...current,
                                    embedding_model: e.target.value,
                                  }
                                : current,
                            )
                          }
                          placeholder={
                            selected.embedding_model || "embedding model"
                          }
                        />
                        <span className="providers-field-hint">
                          {t("providers.embeddingModelHint")}
                        </span>
                      </label>
                    </div>
                  </div>
                )}

                <div className="providers-form-actions">
                  <span
                    className="providers-form-save-state"
                    aria-live="polite"
                  >
                    {hasUnsavedChanges ? t("providers.unsavedChanges") : ""}
                  </span>
                  <Button
                    variant="ghost"
                    size="sm"
                    disabled={!hasUnsavedChanges || saving}
                    onClick={() => {
                      setDraft(draftFromProvider(selected));
                      setError(null);
                    }}
                  >
                    {t("providers.discardChanges")}
                  </Button>
                  <Button
                    variant="primary"
                    size="md"
                    className="providers-save-action"
                    busy={saving}
                    busyLabel={t("providers.saving")}
                    disabled={!hasUnsavedChanges}
                    onClick={() => void saveDraft()}
                  >
                    <IconSave />
                    <span>{t("providers.saveChanges")}</span>
                  </Button>
                </div>
              </div>
            )}

            {error && <p className="providers-error">{error}</p>}
          </section>
        </div>
      )}
    </div>
  );
}
