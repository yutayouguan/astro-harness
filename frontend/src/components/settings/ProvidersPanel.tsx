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
  Box,
  Eye,
  EyeOff,
  Globe,
  GripVertical,
  KeyRound,
  Layers,
  Lightbulb,
  Link2,
  LoaderCircle,
  Plus,
  Power,
  RefreshCw,
  Save,
  Search,
  Star,
  Stethoscope,
  Tag,
  Trash2,
  Waypoints,
  Wrench,
} from "lucide-react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { open as shellOpen } from "@tauri-apps/plugin-shell";
import { ModelBrandIcon, ProviderBrandIcon } from "../icons/ProviderIcons";
import { SelectMenu } from "../ui/SelectMenu";
import { useI18n } from "../../i18n/LocaleContext";
import type { MessageKey } from "../../i18n/messages";
import { EmptyIllustration } from "../../illustrations";
import { formatContextWindow } from "../../lib/model/modelCaps";
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
const MAX_CHAT_FALLBACKS = 3;

/** 规范化草稿中的后备列表（截断 + 空 model → null） */
function normalizeFallback(
  entries: ProviderFallbackEntry[] | undefined | null,
): ProviderFallbackEntry[] {
  return (entries ?? []).slice(0, MAX_CHAT_FALLBACKS).map((e) => {
    const model = e.model?.trim();
    return {
      provider_id: e.provider_id,
      model: model ? model : null,
    };
  });
}

/** 列表状态点：未启动灰 / 健康绿 / 不健康红 */
type HealthStatus = "ok" | "fail" | "checking";

/** Providers 面板入参 */
type Props = {
  /** 面板是否可见 */
  active: boolean;
  /** 配置变更后回传完整状态（供 App 同步 ModelPicker 等） */
  onStateChange?: (state: ProvidersStateDto) => void;
};

const ADD_KINDS: ProviderKindId[] = [
  "anthropic",
  "openai",
  "google",
  "deepseek",
  "azure",
  "zhipu",
  "openrouter",
  "bailian",
  "nvidia",
  "moonshot",
  "volcengine",
  "minimax",
  "ollama",
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
  vision_model: string;
};

type DetailTab = "chat" | "media";

const MEDIA_MODEL_DEFAULTS: Record<
  string,
  { image: string; video: string; tts: string; vision: string }
> = {
  google: {
    image: "gemini-3.1-flash-image",
    video: "veo-3.1-generate-preview",
    tts: "gemini-3.1-flash-tts-preview",
    vision: "gemini-3.5-flash",
  },
  openai: {
    image: "gpt-image-2",
    video: "",
    tts: "gpt-4o-mini-tts",
    vision: "gpt-4o",
  },
};

function supportsMediaModels(kind: string): boolean {
  return kind === "google" || kind === "openai";
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
    vision_model: p.vision_model?.trim() ?? "",
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
    vision_model: draft.vision_model.trim(),
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

/** 显示密钥图标 */
function IconEye(props: SVGProps<SVGSVGElement>) {
  return <Eye size={14} strokeWidth={2} aria-hidden {...props} />;
}

/** 隐藏密钥图标 */
function IconEyeOff(props: SVGProps<SVGSVGElement>) {
  return <EyeOff size={14} strokeWidth={2} aria-hidden {...props} />;
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

/** 联网能力 */
function IconGlobe(props: SVGProps<SVGSVGElement>) {
  return <Globe size={14} strokeWidth={2} aria-hidden {...props} />;
}

/** 推理能力 */
function IconBulb(props: SVGProps<SVGSVGElement>) {
  return <Lightbulb size={14} strokeWidth={2} aria-hidden {...props} />;
}

/** 工具调用能力 */
function IconWrench(props: SVGProps<SVGSVGElement>) {
  return <Wrench size={14} strokeWidth={2} aria-hidden {...props} />;
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

/** 启用/停用 */
function IconPower(props: SVGProps<SVGSVGElement>) {
  return <Power size={16} strokeWidth={2} aria-hidden {...props} />;
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

export default function ProvidersPanel({ active, onStateChange }: Props) {
  const { t } = useI18n();
  const addKindOptions = useMemo(
    () => ADD_KINDS.map((k) => ({ value: k, label: t(kindLabelKey(k)), icon: <ProviderBrandIcon kind={k} /> })),
    [t],
  );
  const [state, setState] = useState<ProvidersStateDto | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [detailTab, setDetailTab] = useState<DetailTab>("chat");
  const [apiKeyInput, setApiKeyInput] = useState("");
  const [storedApiKey, setStoredApiKey] = useState<string | null>(null);
  const [showApiKey, setShowApiKey] = useState(false);
  const [apiKeyDirty, setApiKeyDirty] = useState(false);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testingAll, setTestingAll] = useState(false);
  const [listingModels, setListingModels] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [addKind, setAddKind] = useState<ProviderKindId>("openai");
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [modelsLatency, setModelsLatency] = useState<number | null>(null);
  const [modelLatencies, setModelLatencies] = useState<
    Record<string, ModelLatency>
  >({});
  const [testResult, setTestResult] = useState<ProviderTestResult | null>(null);
  const [modelFilter, setModelFilter] = useState("");
  const [showFilter, setShowFilter] = useState(false);
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
  const healthRunRef = useRef(0);
  const listRef = useRef<HTMLUListElement | null>(null);
  const dragRef = useRef(drag);
  dragRef.current = drag;

  const applyState = useCallback(
    (next: ProvidersStateDto) => {
      setState(next);
      onStateChange?.(next);
    },
    [onStateChange],
  );

  const checkProviderHealth = useCallback(async (provider: ProviderDto, runId?: number) => {
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
  }, []);

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

  const selected =
    state?.providers.find((p) => p.id === selectedId) ?? null;

  useEffect(() => {
    if (!selected) {
      setDraft(null);
      setStoredApiKey(null);
      setApiKeyInput("");
      setShowApiKey(false);
      setApiKeyDirty(false);
      autoFetchIdRef.current = null;
      return;
    }
    setDraft(draftFromProvider(selected));
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
    setShowAddModel(false);
    setCustomModelInput("");
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
    selected?.vision_model,
  ]);

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
      const next = await invoke<ProvidersStateDto>("set_active_provider", { id });
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
      // 先移除再插入时，若向下拖，目标下标需左移一位
      if (cur.fromIndex < to) to -= 1;
      if (to !== cur.fromIndex) {
        void reorderProviders(cur.fromIndex, to);
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
    if (!window.confirm(`${t("providers.delete")} — ${selected.display_name}?`)) {
      return;
    }
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
      setSelectedId(
        next.active_provider_id ??
        next.providers[0]?.id ??
        null,
      );
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

  const listModels = async (opts?: { silent?: boolean; skipSave?: boolean }) => {
    if (!selected || !isTauri()) return;
    const silent = opts?.silent ?? false;
    setListingModels(true);
    if (!silent) setError(null);
    try {
      // 手动拉取时先落盘草稿；自动拉取跳过，避免切提供商时用到旧草稿
      if (draft && !opts?.skipSave) {
        await invoke<ProvidersStateDto>("save_provider", {
          provider: providerSaveInput(selected, draft),
        }).then(applyState);
      }
      const result = await invoke<ProviderModelsResult>("list_provider_models", {
        id: selected.id,
      });
      setModels(result.models);
      setModelsLatency(result.latency_ms);
      setModelLatencies({});
      if (selected.enabled) {
        setHealthById((prev) => ({ ...prev, [selected.id]: "ok" }));
      }
    } catch (err) {
      if (!silent) setError(String(err));
      setModels([]);
      setModelsLatency(null);
      setModelLatencies({});
      if (selected.enabled) {
        setHealthById((prev) => ({ ...prev, [selected.id]: "fail" }));
      }
    } finally {
      setListingModels(false);
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
    void (async () => {
      try {
        const cached = await invoke<ProviderModelsResult | null>(
          "get_cached_provider_models",
          { id: selected.id },
        );
        if (cached && cached.models.length > 0) {
          setModels(cached.models);
          setModelsLatency(cached.latency_ms);
        }
      } catch {
        // ignore cache miss
      }
      await listModels({ silent: true, skipSave: true });
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
      const results = await invoke<ProviderTestResult[]>("test_provider_models", {
        id: selected.id,
        models: models.map((m) => m.id),
      });
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

  const useModel = (modelId: string) => {
    setDraft((d) => (d ? { ...d, model: modelId } : d));
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
                image_gen: false,
                video_gen: false,
                audio_gen: false,
              },
              meta_source: "manual",
            },
            ...prev,
          ],
    );
    useModel(id);
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
  const needsKey = selected?.kind !== "ollama";
  const filteredModels = models.filter((m) => {
    const q = modelFilter.trim().toLowerCase();
    if (!q) return true;
    return (
      m.id.toLowerCase().includes(q) ||
      (m.display_name?.toLowerCase().includes(q) ?? false)
    );
  });
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
        p.id !== selected?.id &&
        !fallbackEntries.some((f) => f.provider_id === p.id),
    ) ?? [];

  const addFallback = (providerId: string) => {
    if (!providerId || fallbackEntries.length >= MAX_CHAT_FALLBACKS) return;
    setDraft((d) =>
      d
        ? {
            ...d,
            fallback: [
              ...d.fallback,
              { provider_id: providerId, model: null },
            ].slice(0, MAX_CHAT_FALLBACKS),
          }
        : d,
    );
  };

  const removeFallback = (index: number) => {
    setDraft((d) =>
      d
        ? { ...d, fallback: d.fallback.filter((_, i) => i !== index) }
        : d,
    );
  };

  const updateFallbackModel = (index: number, model: string) => {
    setDraft((d) => {
      if (!d) return d;
      const trimmed = model.trim();
      const next = d.fallback.map((entry, i) =>
        i === index
          ? { ...entry, model: trimmed ? trimmed : null }
          : entry,
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
    <div className="providers-page" data-tone="blue">
      <div className="providers-layout">
        <aside className="providers-pane providers-pane-list">
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
            {state?.providers.map((p, index) => {
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
                      <ProviderBrandIcon kind={p.kind} />
                    </span>
                    <span className="providers-list-text">
                      <span className="providers-list-name">
                        {p.display_name}
                        {activeDefault && (
                          <span className="providers-badge">
                            {t("providers.active")}
                          </span>
                        )}
                      </span>
                      <span className="providers-list-model">{p.model}</span>
                    </span>
                    <span
                      className={`providers-status-dot ${statusDotClass(p.enabled, healthById[p.id])}`}
                      title={statusDotTitle(p.enabled, healthById[p.id])}
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
                        <ProviderBrandIcon kind={p.kind} />
                      </span>
                      <span className="providers-list-text">
                        <span className="providers-list-name">
                          {p.display_name}
                          {activeDefault && (
                            <span className="providers-badge">
                              {t("providers.active")}
                            </span>
                          )}
                        </span>
                        <span className="providers-list-model">{p.model}</span>
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
            <button
              type="button"
              className="providers-icon-btn providers-add-btn"
              title={t("providers.add")}
              aria-label={t("providers.add")}
              onClick={() => void addProvider()}
            >
              <IconPlus />
            </button>
          </div>
        </aside>

        <section className="providers-pane providers-pane-detail">
          <div className="providers-pane-head">
            <div className="providers-pane-head-text">
              <h2>{t("providers.detailTitle")}</h2>
              <p>{t("providers.detailSub")}</p>
            </div>
            {selected && draft && (
              <div className="providers-pane-head-actions">
                <button
                  type="button"
                  className="providers-icon-btn is-primary"
                  disabled={saving}
                  title={saving ? t("providers.saving") : t("providers.save")}
                  aria-label={saving ? t("providers.saving") : t("providers.save")}
                  onClick={() => void saveDraft()}
                >
                  {saving ? <IconLoader className="is-spin" /> : <IconSave />}
                </button>
                {!isActive && draft.enabled && (
                  <button
                    type="button"
                    className="providers-icon-btn"
                    title={t("providers.setActive")}
                    aria-label={t("providers.setActive")}
                    onClick={() => void setActive(selected.id)}
                  >
                    <IconStar />
                  </button>
                )}
                <button
                  type="button"
                  className="providers-icon-btn is-danger"
                  title={t("providers.delete")}
                  aria-label={t("providers.delete")}
                  onClick={() => void deleteProvider()}
                >
                  <IconTrash />
                </button>
                <button
                  type="button"
                  className={`providers-icon-btn ${draft.enabled ? "is-active" : ""}`}
                  aria-pressed={draft.enabled}
                  title={
                    draft.enabled
                      ? t("providers.enabled")
                      : t("providers.disabled")
                  }
                  aria-label={
                    draft.enabled
                      ? t("providers.enabled")
                      : t("providers.disabled")
                  }
                  disabled={saving}
                  onClick={() => void toggleEnabled()}
                >
                  <IconPower />
                </button>
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
            <div className="providers-form">
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
                  </h3>
                  <p className="providers-form-kind">
                    {t(kindLabelKey(selected.kind))}
                  </p>
                </div>
              </div>

              {supportsMediaModels(selected.kind) ? (
                <div className="providers-detail-tabs" role="tablist">
                  <button
                    type="button"
                    role="tab"
                    aria-selected={detailTab === "chat"}
                    className={`providers-detail-tab ${detailTab === "chat" ? "is-active" : ""}`}
                    onClick={() => setDetailTab("chat")}
                  >
                    {t("providers.tabChat")}
                  </button>
                  <button
                    type="button"
                    role="tab"
                    aria-selected={detailTab === "media"}
                    className={`providers-detail-tab ${detailTab === "media" ? "is-active" : ""}`}
                    onClick={() => setDetailTab("media")}
                  >
                    {t("providers.tabMedia")}
                  </button>
                </div>
              ) : null}

              {(detailTab === "chat" || !supportsMediaModels(selected.kind)) && (
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
                  <p className="providers-field-hint">{t("providers.volcengineModelHint")}</p>
                )}

                <label className="providers-field providers-field-span">
                  <span className="providers-field-label">
                    <IconLink />
                    {t("providers.endpoint")}
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
              </div>

              <div className="providers-fallback-block">
                <div className="providers-fallback-head">
                  <h4 className="providers-block-title">
                    <IconWaypoints />
                    聊天后备
                  </h4>
                  <span className="providers-fallback-count">
                    {fallbackEntries.length}/{MAX_CHAT_FALLBACKS}
                  </span>
                </div>
                <p className="providers-fallback-hint">
                  {fallbackEntries.length === 0
                    ? `失败时按序切换 · 最多 ${MAX_CHAT_FALLBACKS} 个`
                    : `失败时按序切换`}
                </p>
                {fallbackEntries.length > 0 && (
                  <ul className="providers-fallback-list">
                    {fallbackEntries.map((entry, index) => {
                      const options = fallbackModelsById[entry.provider_id] ?? [];
                      const fallbackProvider = state?.providers.find(
                        (p) => p.id === entry.provider_id,
                      );
                      const defaultModel = fallbackProvider?.model?.trim() || "";
                      const selectedModel = entry.model?.trim() || "";
                      const knownIds = new Set(options.map((m) => m.id));
                      const orphanSelected =
                        selectedModel && !knownIds.has(selectedModel)
                          ? selectedModel
                          : null;
                      return (
                      <li key={`${entry.provider_id}-${index}`} className="providers-fallback-row">
                        <span className="providers-fallback-brand" aria-hidden>
                          {fallbackProvider ? (
                            <ProviderBrandIcon kind={fallbackProvider.kind} />
                          ) : (
                            <IconLayers />
                          )}
                        </span>
                        <span className="providers-fallback-name" title={entry.provider_id}>
                          {providerLabel(entry.provider_id)}
                        </span>
                        <SelectMenu
                          className="providers-fallback-model"
                          value={selectedModel}
                          placeholder={t("providers.fallback.defaultModel")}
                          aria-label={t("providers.fallback.modelOverride", { name: providerLabel(entry.provider_id) })}
                          onChange={(v) => updateFallbackModel(index, v)}
                          options={[
                            {
                              value: "",
                              label: defaultModel
                                ? `默认（${defaultModel}）`
                                : "默认模型",
                            },
                            ...(orphanSelected
                              ? [
                                  {
                                    value: orphanSelected,
                                    label: orphanSelected,
                                    icon: (
                                      <ModelBrandIcon modelId={orphanSelected} />
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
                          aria-label={t("providers.fallback.removeAriaLabel", { name: providerLabel(entry.provider_id) })}
                          onClick={() => removeFallback(index)}
                        >
                          <IconTrash />
                        </button>
                      </li>
                      );
                    })}
                  </ul>
                )}
                {fallbackEntries.length < MAX_CHAT_FALLBACKS && (
                  <div className="providers-fallback-add">
                    <span className="providers-fallback-add-icon" aria-hidden>
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
                    <button
                      type="button"
                      className="providers-key-link"
                      onClick={() => void openOfficialKey()}
                    >
                      {t("providers.getOfficialKey")}
                    </button>
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
                            {showApiKey ? <IconEyeOff /> : <IconEye />}
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
                        {saving ? <IconLoader className="is-spin" /> : <IconSave />}
                      </button>
                      <button
                        type="button"
                        className={`providers-icon-btn ${testing ? "is-busy" : ""}`}
                        disabled={testing || !selected.has_api_key}
                        title={testing ? t("providers.testing") : t("providers.test")}
                        aria-label={testing ? t("providers.testing") : t("providers.test")}
                        aria-busy={testing}
                        onClick={() => void testConnection()}
                      >
                        {testing ? <IconLoader className="is-spin" /> : <IconStethoscope />}
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
                      <span className="providers-models-latency">
                        {t("providers.modelsLatency", {
                          ms: String(modelsLatency),
                        })}
                      </span>
                    )}
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
                  </div>
                  <div className="providers-models-toolbar">
                    {!needsKey && (
                      <button
                        type="button"
                        className={`providers-icon-btn ${testing ? "is-busy" : ""}`}
                        disabled={testing || testingAll}
                        title={testing ? t("providers.testing") : t("providers.test")}
                        aria-label={testing ? t("providers.testing") : t("providers.test")}
                        aria-busy={testing}
                        onClick={() => void testConnection()}
                      >
                        {testing ? <IconLoader className="is-spin" /> : <IconStethoscope />}
                      </button>
                    )}
                    <button
                      type="button"
                      className={`providers-icon-btn is-primary ${listingModels ? "is-busy" : ""}`}
                      disabled={
                        listingModels || (needsKey && !selected.has_api_key)
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
                      onChange={(e) => setCustomModelInput(e.target.value)}
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
                      const ctxLabel = formatContextWindow(m.context_window);
                      return (
                        <li
                          key={m.id}
                          className={`providers-model-row ${isCurrent ? "is-current" : ""}`}
                        >
                          <label className="providers-model-pick">
                            <input
                              type="radio"
                              className="providers-model-radio"
                              name="providers-model-select"
                              checked={isCurrent}
                              onChange={() => useModel(m.id)}
                              title={t("providers.useModel")}
                              aria-label={`${t("providers.useModel")}: ${m.id}`}
                            />
                            <ModelBrandIcon modelId={m.id} className="providers-model-icon" />
                            <span className="providers-model-id" title={m.display_name ?? m.id}>
                              {m.id}
                            </span>
                            {ctxLabel && (
                              <span
                                className="providers-model-ctx"
                                title={t("providers.contextWindow")}
                              >
                                {ctxLabel}
                              </span>
                            )}
                            {latency && (
                              <span
                                className={`providers-model-latency ${latency.ok ? "ok" : "fail"}`}
                              >
                                {latency.latency_ms < 0
                                  ? "—"
                                  : `${latency.latency_ms} ms`}
                              </span>
                            )}
                          </label>
                          <span className="providers-model-caps" aria-label="capabilities">
                            {caps.vision && (
                              <span className="providers-cap vision" title={t("providers.cap.vision")}>
                                <IconEye />
                              </span>
                            )}
                            {caps.web && (
                              <span className="providers-cap web" title={t("providers.cap.web")}>
                                <IconGlobe />
                              </span>
                            )}
                            {caps.reasoning && (
                              <span className="providers-cap reasoning" title={t("providers.cap.reasoning")}>
                                <IconBulb />
                              </span>
                            )}
                            {caps.tools && (
                              <span className="providers-cap tools" title={t("providers.cap.tools")}>
                                <IconWrench />
                              </span>
                            )}
                          </span>
                          <span className="providers-model-actions">
                            <button
                              type="button"
                              className={`providers-icon-btn ${testing && isCurrent ? "is-busy" : ""}`}
                              disabled={testing || testingAll}
                              title={t("providers.test")}
                              aria-label={`${t("providers.test")}: ${m.id}`}
                              onClick={() => {
                                useModel(m.id);
                                void testConnection(m.id);
                              }}
                            >
                              <IconStethoscope />
                            </button>
                          </span>
                        </li>
                      );
                    })}
                  </ul>
                )}
              </div>
              </>
              )}

              {detailTab === "media" && supportsMediaModels(selected.kind) && (
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
                      <input
                        type="text"
                        value={draft.image_model}
                        placeholder={
                          MEDIA_MODEL_DEFAULTS[selected.kind]?.image ?? ""
                        }
                        onChange={(e) =>
                          setDraft((d) =>
                            d ? { ...d, image_model: e.target.value } : d,
                          )
                        }
                      />
                    </label>
                    {selected.kind === "google" ? (
                      <label className="providers-field providers-field-span">
                        <span className="providers-field-label">
                          <IconBox />
                          {t("providers.videoModel")}
                        </span>
                        <input
                          type="text"
                          value={draft.video_model}
                          placeholder={
                            MEDIA_MODEL_DEFAULTS.google?.video ?? ""
                          }
                          onChange={(e) =>
                            setDraft((d) =>
                              d ? { ...d, video_model: e.target.value } : d,
                            )
                          }
                        />
                      </label>
                    ) : null}
                    <label className="providers-field providers-field-span">
                      <span className="providers-field-label">
                        <IconBox />
                        {t("providers.ttsModel")}
                      </span>
                      <input
                        type="text"
                        value={draft.tts_model}
                        placeholder={
                          MEDIA_MODEL_DEFAULTS[selected.kind]?.tts ?? ""
                        }
                        onChange={(e) =>
                          setDraft((d) =>
                            d ? { ...d, tts_model: e.target.value } : d,
                          )
                        }
                      />
                    </label>
                    <label className="providers-field providers-field-span">
                      <span className="providers-field-label">
                        <IconBox />
                        {t("providers.visionModel")}
                      </span>
                      <input
                        type="text"
                        value={draft.vision_model}
                        placeholder={
                          MEDIA_MODEL_DEFAULTS[selected.kind]?.vision ?? ""
                        }
                        onChange={(e) =>
                          setDraft((d) =>
                            d ? { ...d, vision_model: e.target.value } : d,
                          )
                        }
                      />
                    </label>
                  </div>
                </div>
              )}
            </div>
          )}

          {error && <p className="providers-error">{error}</p>}
        </section>
      </div>
    </div>
  );
}
