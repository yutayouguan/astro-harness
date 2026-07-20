import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  AlertTriangle,
  Bot,
  Brain,
  ClipboardCheck,
  MoonStar,
  RefreshCw,
  RotateCcw,
  Sparkles,
  WandSparkles,
  type LucideIcon,
} from "lucide-react";
import { useAuxiliarySettings } from "../../hooks/settings/useAuxiliarySettings";
import { useI18n } from "../../i18n/LocaleContext";
import type { MessageKey } from "../../i18n/messages";
import type {
  AuxiliaryTaskDto,
  AuxiliaryTaskId,
  ModelInfo,
  ProviderDto,
  ProviderModelsResult,
  ProvidersStateDto,
} from "../../types";
import { SelectMenu } from "../ui/SelectMenu";

type Props = {
  active: boolean;
  /** 嵌在模型服务页 Tab 内时隐藏顶部 hero，操作并入列表头 */
  embedded?: boolean;
};

const TASKS: {
  id: AuxiliaryTaskId;
  labelKey: MessageKey;
  descKey: MessageKey;
  Icon: LucideIcon;
}[] = [
  {
    id: "title_generation",
    labelKey: "aux.titleGeneration",
    descKey: "aux.titleGenerationDesc",
    Icon: WandSparkles,
  },
  {
    id: "compaction",
    labelKey: "aux.compaction",
    descKey: "aux.compactionDesc",
    Icon: Brain,
  },
  {
    id: "smart_approval",
    labelKey: "aux.smartApproval",
    descKey: "aux.smartApprovalDesc",
    Icon: ClipboardCheck,
  },
  {
    id: "dreaming",
    labelKey: "aux.dreaming",
    descKey: "aux.dreamingDesc",
    Icon: MoonStar,
  },
  {
    id: "background_review",
    labelKey: "aux.backgroundReview",
    descKey: "aux.backgroundReviewDesc",
    Icon: Sparkles,
  },
];

function ensureDefaultModel(models: ModelInfo[], model: string): ModelInfo[] {
  const id = model.trim();
  if (!id) return models;
  return models.some((m) => m.id === id)
    ? models
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
        },
        ...models,
      ];
}

export default function AuxiliaryModelsPanel({ active, embedded = false }: Props) {
  const { t } = useI18n();
  const {
    loading,
    error,
    settings,
    setRoute,
    resetRoute,
    resetAll,
    reload,
  } = useAuxiliarySettings(active);
  const [providersState, setProvidersState] = useState<ProvidersStateDto | null>(null);
  const [providersLoading, setProvidersLoading] = useState(false);
  const [providersError, setProvidersError] = useState<string | null>(null);
  const [editingTask, setEditingTask] = useState<AuxiliaryTaskId | null>(null);
  const [selectedProviderId, setSelectedProviderId] = useState("");
  const [modelsByProvider, setModelsByProvider] = useState<Record<string, ModelInfo[]>>({});
  const [loadingModelsFor, setLoadingModelsFor] = useState<string | null>(null);

  const enabledProviders = useMemo(
    () => (providersState?.providers ?? []).filter((p) => p.enabled),
    [providersState],
  );

  /** 自动路由实际生效的主模型（provider 展示名 + model id） */
  const primaryRoute = useMemo(() => {
    const providerId =
      settings?.activeProviderId ?? providersState?.active_provider_id ?? null;
    if (!providerId) return null;
    const provider = (providersState?.providers ?? []).find((p) => p.id === providerId);
    const model = (settings?.activeModel || provider?.model || "").trim();
    if (!provider || !model) return null;
    return {
      id: provider.id,
      name: provider.display_name || provider.id,
      model,
    };
  }, [providersState, settings?.activeModel, settings?.activeProviderId]);

  const taskRows = useMemo(() => {
    const byId = new Map((settings?.tasks ?? []).map((task) => [task.id, task]));
    return TASKS.map((task) => byId.get(task.id)).filter(Boolean) as AuxiliaryTaskDto[];
  }, [settings]);

  const resolveRouteLabel = useCallback(
    (row: AuxiliaryTaskDto | undefined, isAuto: boolean) => {
      if (isAuto) {
        if (primaryRoute) {
          return t("aux.inherits", {
            provider: primaryRoute.name,
            model: primaryRoute.model,
          });
        }
        return t("aux.inheritsUnknown");
      }
      return row?.displayLabel ?? t("aux.followPrimary");
    },
    [primaryRoute, t],
  );

  const usesPrimaryModel = useCallback(
    (row: AuxiliaryTaskDto | undefined, isAuto: boolean) => {
      if (isAuto) return Boolean(primaryRoute);
      if (!row || !primaryRoute) return false;
      return row.provider === primaryRoute.id && row.model === primaryRoute.model;
    },
    [primaryRoute],
  );

  const providerOptions = useMemo(
    () =>
      enabledProviders.map((p) => ({
        value: p.id,
        label: p.display_name,
      })),
    [enabledProviders],
  );

  const selectedProvider = enabledProviders.find((p) => p.id === selectedProviderId);
  const selectedModels = selectedProvider
    ? ensureDefaultModel(modelsByProvider[selectedProvider.id] ?? [], selectedProvider.model)
    : [];
  const modelOptions = selectedModels.map((m) => ({
    value: m.id,
    label: m.display_name ? `${m.display_name} · ${m.id}` : m.id,
  }));

  const loadProviders = useCallback(async () => {
    if (!active) return;
    setProvidersLoading(true);
    setProvidersError(null);
    try {
      const next = await invoke<ProvidersStateDto>("get_providers_state");
      setProvidersState(next);
    } catch (err) {
      setProvidersError(err instanceof Error ? err.message : String(err));
    } finally {
      setProvidersLoading(false);
    }
  }, [active]);

  useEffect(() => {
    void loadProviders();
  }, [loadProviders]);

  const loadModels = useCallback(
    async (provider: ProviderDto) => {
      setLoadingModelsFor(provider.id);
      try {
        let models: ModelInfo[] = [];
        try {
          const cached = await invoke<ProviderModelsResult | null>(
            "get_cached_provider_models",
            { id: provider.id },
          );
          if (cached?.models?.length) models = cached.models;
        } catch {
          // Cache miss is expected for newly added providers.
        }
        if (models.length === 0) {
          const fresh = await invoke<ProviderModelsResult>("list_provider_models", {
            id: provider.id,
          });
          models = fresh.models ?? [];
        }
        setModelsByProvider((prev) => ({
          ...prev,
          [provider.id]: ensureDefaultModel(models, provider.model),
        }));
      } catch (err) {
        setProvidersError(err instanceof Error ? err.message : String(err));
        setModelsByProvider((prev) => ({
          ...prev,
          [provider.id]: ensureDefaultModel([], provider.model),
        }));
      } finally {
        setLoadingModelsFor(null);
      }
    },
    [],
  );

  const beginEdit = useCallback(
    (row: AuxiliaryTaskDto) => {
      const nextProviderId =
        row.provider !== "auto"
          ? row.provider
          : providersState?.active_provider_id ?? enabledProviders[0]?.id ?? "";
      setEditingTask(row.id);
      setSelectedProviderId(nextProviderId);
      const provider = enabledProviders.find((p) => p.id === nextProviderId);
      if (provider) void loadModels(provider);
    },
    [enabledProviders, loadModels, providersState?.active_provider_id],
  );

  const handleProviderChange = useCallback(
    (providerId: string) => {
      setSelectedProviderId(providerId);
      const provider = enabledProviders.find((p) => p.id === providerId);
      if (provider) void loadModels(provider);
    },
    [enabledProviders, loadModels],
  );

  const handleModelChange = useCallback(
    async (model: string) => {
      if (!editingTask || !selectedProviderId) return;
      await setRoute(editingTask, selectedProviderId, model);
      setEditingTask(null);
    },
    [editingTask, selectedProviderId, setRoute],
  );

  const actionButtons = (
    <>
      <button
        type="button"
        className="aux-action aux-action-ghost"
        onClick={() => void reload()}
        disabled={loading}
      >
        <RefreshCw size={15} />
        {t("aux.refresh")}
      </button>
      <button
        type="button"
        className="aux-action"
        onClick={() => {
          void resetAll();
          setEditingTask(null);
        }}
        disabled={loading}
      >
        <RotateCcw size={15} />
        {t("aux.resetAll")}
      </button>
    </>
  );

  return (
    <div
      className={`aux-page${embedded ? " aux-page-embedded" : ""}`}
      data-tone={embedded ? "blue" : "purple"}
    >
      {!embedded && (
        <section className="prefs-card aux-hero">
          <div className="prefs-card-head">
            <div className="prefs-icon-badge">
              <Bot size={21} />
            </div>
            <div>
              <h2 className="prefs-card-title">{t("aux.title")}</h2>
              <p className="prefs-card-sub">{t("aux.subtitle")}</p>
            </div>
          </div>
          <div className="aux-hero-actions">{actionButtons}</div>
        </section>
      )}

      {(error || providersError) && (
        <div className="aux-error">
          <AlertTriangle size={16} />
          {error ?? providersError}
        </div>
      )}

      <section className="prefs-card aux-list-card">
        <div className="aux-list-head">
          <div>
            <h2 className="prefs-card-title">{t("aux.routesTitle")}</h2>
            <p className="prefs-card-sub">
              {embedded ? t("aux.subtitle") : t("aux.routesSub")}
            </p>
          </div>
          <div className="aux-list-head-actions">
            {providersLoading && <span className="aux-muted">{t("aux.loading")}</span>}
            {embedded ? actionButtons : null}
          </div>
        </div>

        <div className="aux-task-list">
          {TASKS.map((task) => {
            const row = taskRows.find((item) => item.id === task.id);
            const Icon = task.Icon;
            const isEditing = editingTask === task.id;
            const isAuto = !row || row.provider === "auto";
            return (
              <article className="aux-task-row" key={task.id}>
                <div className="aux-task-icon">
                  <Icon size={18} />
                </div>
                <div className="aux-task-main">
                  <div className="aux-task-titleline">
                    <h3>{t(task.labelKey)}</h3>
                    <span className={`aux-route-pill${isAuto ? " is-auto" : ""}`}>
                      {isAuto ? t("aux.auto") : t("aux.custom")}
                    </span>
                  </div>
                  <p>{t(task.descKey)}</p>
                  <div className="aux-task-current">
                    <span>{resolveRouteLabel(row, isAuto)}</span>
                    {row?.unavailable && (
                      <span className="aux-warning">
                        <AlertTriangle size={13} />
                        {t("aux.unavailable")}
                      </span>
                    )}
                  </div>
                  {(task.id === "compaction" ||
                    task.id === "background_review" ||
                    usesPrimaryModel(row, isAuto)) && (
                    <div className="aux-task-hints">
                      {task.id === "compaction" && (
                        <span className="aux-hint">{t("aux.compactionThresholdHint")}</span>
                      )}
                      {task.id === "background_review" && (
                        <span className="aux-hint">{t("aux.backgroundReviewHint")}</span>
                      )}
                      {usesPrimaryModel(row, isAuto) && (
                        <span className="aux-hint aux-hint-cost">
                          {t("aux.costSameAsPrimary")}
                        </span>
                      )}
                    </div>
                  )}

                  {isEditing && (
                    <div className="aux-editor">
                      <SelectMenu
                        value={selectedProviderId}
                        options={providerOptions}
                        onChange={handleProviderChange}
                        disabled={providerOptions.length === 0}
                        placeholder={t("aux.selectProvider")}
                        aria-label={t("aux.selectProvider")}
                      />
                      <SelectMenu
                        value=""
                        options={modelOptions}
                        onChange={handleModelChange}
                        disabled={!selectedProvider || loadingModelsFor === selectedProvider?.id}
                        placeholder={
                          loadingModelsFor === selectedProvider?.id
                            ? t("aux.loadingModels")
                            : t("aux.selectModel")
                        }
                        aria-label={t("aux.selectModel")}
                      />
                      <button
                        type="button"
                        className="aux-action aux-action-ghost"
                        onClick={() => setEditingTask(null)}
                      >
                        {t("aux.cancel")}
                      </button>
                    </div>
                  )}
                </div>
                <div className="aux-task-actions">
                  <button
                    type="button"
                    className="aux-action aux-action-ghost"
                    onClick={() => {
                      void resetRoute(task.id);
                      if (editingTask === task.id) setEditingTask(null);
                    }}
                    disabled={isAuto}
                  >
                    {t("aux.usePrimary")}
                  </button>
                  <button
                    type="button"
                    className="aux-action"
                    onClick={() => {
                      if (!providersState) void loadProviders();
                      beginEdit(
                        row ?? {
                          id: task.id,
                          provider: "auto",
                          model: "auto",
                          displayLabel: resolveRouteLabel(undefined, true),
                          unavailable: false,
                        },
                      );
                    }}
                    disabled={enabledProviders.length === 0}
                  >
                    {t("aux.change")}
                  </button>
                </div>
              </article>
            );
          })}
        </div>
      </section>
    </div>
  );
}

export { TASKS as AUXILIARY_TASKS };
