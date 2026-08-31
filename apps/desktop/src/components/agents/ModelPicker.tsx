/** 模型选择器。 */
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type MouseEvent as ReactMouseEvent,
} from "react";
import { createPortal } from "react-dom";
import { Check } from "lucide-react";
import {
  ChevronDown as ChevronDownData,
  ChevronRight as ChevronRightData,
  ChevronUp as ChevronUpData,
} from "lucide";
import { invoke } from "@tauri-apps/api/core";
import { useClampPopover } from "../../hooks/ui/useClampPopover";
import { useConfirm } from "../../hooks/ui/DialogContext";
import { useI18n } from "../../i18n/LocaleContext";
import {
  DEFAULT_MODEL_PREFS,
  clampPrefsToModelConfig,
  contextChoicesForWindow,
  effortChoicesFromMeta,
  loadAllModelPrefs,
  loadModelPrefs,
  loadPickerGlobals,
  modelPrefsKey,
  modelSupportsReasoning,
  upsertModelPrefs,
  type ModelContextSize,
  type ModelEffort,
  type ModelPickerGlobals,
  type ModelRuntimePrefs,
} from "../../lib/model/modelPrefs";
import {
  EMPTY_MODEL_CAPABILITIES,
  compareModelsByCreatedDesc,
} from "../../lib/model/modelCaps";
import type {
  ModelCapabilities,
  ModelInfo,
  ModelReasoningMeta,
  ProviderDto,
  ProviderModelsResult,
} from "../../types";
import ModelCapabilityIcons from "./ModelCapabilityIcons";
import { ModelBrandIcon } from "../icons/ProviderIcons";
import { MorphToggleIcon } from "../icons/MorphIcon";

/** 模型选择器入参 */
type Props = {
  providers: ProviderDto[];
  /** 当前活跃提供商 id */
  value: string | null;
  /** 切换提供商与模型 */
  onChange: (providerId: string, model: string) => void;
  disabled?: boolean;
  /** 当前模型配置变更（便于 App 同步发送参数） */
  onActivePrefsChange?: (
    prefs: ModelRuntimePrefs,
    globals: ModelPickerGlobals,
  ) => void;
};

/** 下拉中的一项：某提供商下的模型 */
type ModelOption = {
  providerId: string;
  providerName: string;
  modelId: string;
  capabilities: ModelCapabilities;
  expirationDate?: string | null;
  created?: number | null;
  contextWindow?: number | null;
  reasoning?: ModelReasoningMeta | null;
};

/** 选中勾选图标 */
function IconCheck(props: { className?: string }) {
  return <Check size={14} strokeWidth={2.5} aria-hidden {...props} />;
}

/** 确保默认模型 id 出现在列表中（缺失则插入占位项） */
function ensureDefaultModel(
  models: ModelInfo[],
  defaultId: string,
): ModelInfo[] {
  const id = defaultId.trim();
  if (!id) return models;
  if (models.some((m) => m.id === id)) return models;
  return [
    {
      id,
      capabilities: { ...EMPTY_MODEL_CAPABILITIES, tools: true },
      meta_source: "default",
    },
    ...models,
  ];
}

/** 能力开关（视觉/联网等） */
function ToggleSwitch({
  checked,
  onChange,
  label,
  disabled,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <label className={`mp-toggle ${disabled ? "is-disabled" : ""}`}>
      <span className="mp-toggle-label">{label}</span>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        aria-label={label}
        disabled={disabled}
        className={`mp-switch ${checked ? "is-on" : ""}`}
        onClick={(e) => {
          e.stopPropagation();
          onChange(!checked);
        }}
      >
        <span className="mp-switch-knob" />
      </button>
    </label>
  );
}

const COLLAPSED_GROUPS_KEY = "astro.modelPicker.collapsedGroups";

function loadCollapsedGroups(): Set<string> {
  try {
    const raw = localStorage.getItem(COLLAPSED_GROUPS_KEY);
    if (!raw) return new Set();
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed)) return new Set();
    return new Set(parsed.filter((id): id is string => typeof id === "string"));
  } catch {
    return new Set();
  }
}

function saveCollapsedGroups(ids: Set<string>) {
  try {
    localStorage.setItem(COLLAPSED_GROUPS_KEY, JSON.stringify([...ids]));
  } catch {
    /* ignore quota */
  }
}

/** 聊天顶栏：列出各已启用提供商的全部可用模型（缓存），选中即切换提供商并设为默认模型 */
export default function ModelPicker({
  providers,
  value,
  onChange,
  disabled = false,
  onActivePrefsChange,
}: Props) {
  const { t } = useI18n();
  const confirm = useConfirm();
  const [open, setOpen] = useState(false);
  const [options, setOptions] = useState<ModelOption[]>([]);
  const [loading, setLoading] = useState(false);
  const [editing, setEditing] = useState<ModelOption | null>(null);
  const [editPrefs, setEditPrefs] = useState<ModelRuntimePrefs>({
    ...DEFAULT_MODEL_PREFS,
  });
  const [globals] = useState<ModelPickerGlobals>(() => loadPickerGlobals());
  const [collapsedGroups, setCollapsedGroups] = useState<Set<string>>(() =>
    loadCollapsedGroups(),
  );
  const [, setPrefsTick] = useState(0);
  const ref = useRef<HTMLDivElement | null>(null);
  const flyoutRef = useRef<HTMLDivElement | null>(null);
  const activeProvider =
    providers.find((p) => p.id === value) ?? providers[0] ?? null;

  const activeModelId = activeProvider?.model ?? "";

  const notifyActive = useCallback(
    (g: ModelPickerGlobals = globals) => {
      if (!activeProvider || !onActivePrefsChange) return;
      const prefs = loadModelPrefs(activeProvider.id, activeProvider.model);
      onActivePrefsChange(prefs, g);
    },
    [activeProvider, globals, onActivePrefsChange],
  );

  useEffect(() => {
    notifyActive();
  }, [notifyActive, activeModelId, value]);

  const loadOptions = useCallback(
    async (refreshEmpty = false) => {
      if (providers.length === 0) {
        setOptions([]);
        return;
      }
      setLoading(true);
      try {
        const groups = await Promise.all(
          providers.map(async (p) => {
            let models: ModelInfo[] = [];
            try {
              const cached = await invoke<ProviderModelsResult | null>(
                "get_cached_provider_models",
                { id: p.id },
              );
              if (cached?.models?.length) models = cached.models;
            } catch {
              // ignore cache miss
            }

            const onlyDefault =
              models.length === 0 ||
              (models.length === 1 && models[0]?.id === p.model);
            if (refreshEmpty && onlyDefault) {
              try {
                const fresh = await invoke<ProviderModelsResult>(
                  "list_provider_models",
                  { id: p.id },
                );
                if (fresh.models?.length) models = fresh.models;
              } catch {
                // 拉取失败则继续用缓存 / 默认
              }
            }

            models = ensureDefaultModel(models, p.model);
            models = [...models].sort(compareModelsByCreatedDesc);
            return models.map((m) => ({
              providerId: p.id,
              providerName: p.display_name,
              modelId: m.id,
              capabilities: m.capabilities ?? {
                ...EMPTY_MODEL_CAPABILITIES,
                tools: true,
              },
              expirationDate: m.expiration_date ?? null,
              created: m.created ?? null,
              contextWindow: m.context_window ?? null,
              reasoning: m.reasoning ?? null,
            }));
          }),
        );
        setOptions(groups.flat());
      } finally {
        setLoading(false);
      }
    },
    [providers],
  );

  useEffect(() => {
    void loadOptions(false);
  }, [loadOptions]);

  useEffect(() => {
    if (!open) {
      setEditing(null);
      return;
    }
    void loadOptions(true);
  }, [open, loadOptions]);

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: globalThis.MouseEvent) => {
      const target = e.target as Node;
      if (ref.current?.contains(target) || flyoutRef.current?.contains(target))
        return;
      setOpen(false);
      setEditing(null);
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [open]);

  const clampedStyle = useClampPopover({
    open,
    anchorRef: ref,
    popoverRef: flyoutRef,
    sizeKey: `${editing ? `${editing.providerId}:${editing.modelId}` : ""}:${options.length}`,
    mode: "fixed",
    preferAlign: "end",
    placement: "below",
    gap: 8,
  });
  const flyoutStyle: CSSProperties = clampedStyle ?? { visibility: "hidden" };

  const grouped = useMemo(() => {
    const map = new Map<
      string,
      { providerId: string; name: string; items: ModelOption[] }
    >();
    for (const opt of options) {
      let g = map.get(opt.providerId);
      if (!g) {
        g = {
          providerId: opt.providerId,
          name: opt.providerName,
          items: [],
        };
        map.set(opt.providerId, g);
      }
      g.items.push(opt);
    }
    return [...map.values()].map((g) => ({
      providerId: g.providerId,
      name: g.name,
      items: g.items,
    }));
  }, [options]);

  // 打开时确保当前选中模型所在分组展开
  useEffect(() => {
    if (!open || !activeProvider) return;
    setCollapsedGroups((prev) => {
      if (!prev.has(activeProvider.id)) return prev;
      const next = new Set(prev);
      next.delete(activeProvider.id);
      saveCollapsedGroups(next);
      return next;
    });
  }, [open, activeProvider]);

  const toggleGroup = useCallback((providerId: string) => {
    setCollapsedGroups((prev) => {
      const next = new Set(prev);
      if (next.has(providerId)) next.delete(providerId);
      else next.add(providerId);
      saveCollapsedGroups(next);
      return next;
    });
  }, []);

  const openEdit = (
    opt: ModelOption,
    e: ReactMouseEvent<HTMLButtonElement>,
  ) => {
    e.stopPropagation();
    e.preventDefault();
    const prefs = clampPrefsToModelConfig(
      loadModelPrefs(opt.providerId, opt.modelId),
      {
        capsReasoning: opt.capabilities.reasoning,
        reasoning: opt.reasoning,
        contextWindow: opt.contextWindow,
      },
    );
    setEditPrefs(prefs);
    setEditing(opt);
  };

  const patchEdit = (patch: Partial<ModelRuntimePrefs>) => {
    if (!editing) return;
    const raw = upsertModelPrefs(editing.providerId, editing.modelId, patch);
    const next = clampPrefsToModelConfig(raw, {
      capsReasoning: editing.capabilities.reasoning,
      reasoning: editing.reasoning,
      contextWindow: editing.contextWindow,
    });
    if (
      next.thinking !== raw.thinking ||
      next.effort !== raw.effort ||
      next.context !== raw.context ||
      next.fast !== raw.fast
    ) {
      upsertModelPrefs(editing.providerId, editing.modelId, next);
    }
    setEditPrefs(next);
    setPrefsTick((n) => n + 1);
    if (
      editing.providerId === activeProvider?.id &&
      editing.modelId === activeModelId
    ) {
      onActivePrefsChange?.(next, globals);
    }
  };

  const editEfforts = editing
    ? effortChoicesFromMeta(editing.reasoning, editing.capabilities.reasoning)
    : [];
  const editContexts = editing
    ? contextChoicesForWindow(editing.contextWindow)
    : [];
  const editSupportsReasoning = editing
    ? modelSupportsReasoning(editing.capabilities.reasoning, editing.reasoning)
    : false;
  const editThinkingMandatory = Boolean(editing?.reasoning?.mandatory);

  const effortLabel = (e: ModelEffort) => {
    switch (e) {
      case "none":
        return t("modelEdit.effortNone");
      case "minimal":
        return t("modelEdit.effortMinimal");
      case "low":
        return t("modelEdit.effortLow");
      case "medium":
        return t("modelEdit.effortMedium");
      case "high":
        return t("modelEdit.effortHigh");
      case "xhigh":
        return t("modelEdit.effortXHigh");
      case "max":
        return t("modelEdit.effortMax");
    }
  };

  const contextLabel = (c: ModelContextSize) => {
    if (c === "300k") return "300K";
    if (c === "1m") return "1M";
    return c;
  };

  if (providers.length === 0) {
    return (
      <div className="model-picker is-empty">
        <span className="model-picker-label">{t("status.none")}</span>
      </div>
    );
  }

  return (
    <div className={`model-picker ${open ? "is-open" : ""}`} ref={ref}>
      <button
        type="button"
        className="model-picker-trigger"
        disabled={disabled}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={t("chat.selectModel")}
        onClick={() => setOpen((v) => !v)}
      >
        {activeProvider ? (
          <>
            <span className="model-picker-icon" aria-hidden>
              <ModelBrandIcon modelId={activeModelId} />
            </span>
            <span className="model-picker-label" title={activeModelId}>
              {activeModelId || t("status.none")}
            </span>
          </>
        ) : (
          <span className="model-picker-label">{t("status.none")}</span>
        )}
        <MorphToggleIcon
          active={open}
          activeIcon={ChevronUpData}
          inactiveIcon={ChevronDownData}
          size={12}
          strokeWidth={2.5}
          className="model-picker-chevron"
          aria-hidden
        />
      </button>

      {open && typeof document !== "undefined"
        ? createPortal(
            <div
              ref={flyoutRef}
              className={`model-picker-flyout ${editing ? "has-edit" : ""}`}
              style={flyoutStyle}
            >
              <div className="model-picker-panel model-picker-panel--list">
                <ul
                  className="model-picker-menu"
                  role="listbox"
                  aria-label={t("chat.selectModel")}
                >
                  {loading && options.length === 0 && (
                    <li className="model-picker-empty">
                      {t("providers.listingModels")}
                    </li>
                  )}
                  {!loading && options.length === 0 && (
                    <li className="model-picker-empty">
                      {t("providers.modelsEmpty")}
                    </li>
                  )}
                  {grouped.map((group) => {
                    const collapsed = collapsedGroups.has(group.providerId);
                    return (
                      <li
                        key={group.providerId}
                        className={`model-picker-group ${collapsed ? "is-collapsed" : ""}`}
                        role="presentation"
                      >
                        <button
                          type="button"
                          className="model-picker-group-label"
                          aria-expanded={!collapsed}
                          aria-controls={`model-picker-group-${group.providerId}`}
                          aria-label={`${t("chat.modelGroupToggle")}: ${group.name}`}
                          onClick={() => toggleGroup(group.providerId)}
                        >
                          <span className="model-picker-group-label-text">
                            {group.name}
                            <span className="model-picker-group-count">
                              {group.items.length}
                            </span>
                          </span>
                          <MorphToggleIcon
                            active={!collapsed}
                            activeIcon={ChevronDownData}
                            inactiveIcon={ChevronRightData}
                            size={12}
                            strokeWidth={2.4}
                            className="model-picker-group-chevron"
                            aria-hidden
                          />
                        </button>
                        {!collapsed ? (
                          <ul
                            id={`model-picker-group-${group.providerId}`}
                            className="model-picker-group-list"
                            role="group"
                          >
                            {group.items.map((opt) => {
                              const selected =
                                opt.providerId === activeProvider?.id &&
                                opt.modelId === activeModelId;
                              const editingThis =
                                editing?.providerId === opt.providerId &&
                                editing?.modelId === opt.modelId;
                              const prefs =
                                loadAllModelPrefs()[
                                  modelPrefsKey(opt.providerId, opt.modelId)
                                ];
                              const badge =
                                prefs?.thinking === false
                                  ? t("modelEdit.badgeOff")
                                  : prefs?.effort === "max" ||
                                      prefs?.effort === "xhigh"
                                    ? t("modelEdit.badgeMax")
                                    : prefs?.effort === "high"
                                      ? t("modelEdit.badgeHigh")
                                      : null;
                              return (
                                <li
                                  key={`${opt.providerId}:${opt.modelId}`}
                                  role="option"
                                  aria-selected={selected}
                                  className="model-picker-option-row"
                                >
                                  <button
                                    type="button"
                                    className={`model-picker-option ${
                                      selected ? "is-selected" : ""
                                    } ${editingThis ? "is-editing" : ""}`}
                                    onClick={() => {
                                      void (async () => {
                                        if (selected) {
                                          setOpen(false);
                                          setEditing(null);
                                          return;
                                        }
                                        const exp = opt.expirationDate?.trim();
                                        if (exp) {
                                          const ok = await confirm({
                                            title: t(
                                              "providers.expiringConfirmTitle",
                                            ),
                                            message: t(
                                              "providers.expiringConfirmMessage",
                                            ),
                                            emphasis: opt.modelId,
                                            emphasisLabel: `${t("providers.expiration")}: ${exp}`,
                                            confirmLabel: t(
                                              "providers.expiringConfirmOk",
                                            ),
                                            variant: "danger",
                                          });
                                          if (!ok) return;
                                        }
                                        onChange(opt.providerId, opt.modelId);
                                        setOpen(false);
                                        setEditing(null);
                                      })();
                                    }}
                                  >
                                    <span
                                      className="model-picker-option-icon"
                                      aria-hidden
                                    >
                                      <ModelBrandIcon modelId={opt.modelId} />
                                    </span>
                                    <span className="model-picker-option-text">
                                      <span className="model-picker-option-model">
                                        {opt.modelId}
                                        {badge ? (
                                          <span className="model-picker-option-badge">
                                            {badge}
                                          </span>
                                        ) : null}
                                      </span>
                                      <span className="model-picker-option-meta">
                                        <span className="model-picker-option-provider">
                                          {opt.providerName}
                                        </span>
                                        <ModelCapabilityIcons
                                          capabilities={opt.capabilities}
                                        />
                                      </span>
                                    </span>
                                    <span
                                      className={`model-picker-option-check ${
                                        selected ? "is-visible" : ""
                                      }`}
                                      aria-hidden={!selected}
                                    >
                                      {selected ? <IconCheck /> : null}
                                    </span>
                                  </button>
                                  <span className="model-picker-option-actions">
                                    <button
                                      type="button"
                                      className={`model-picker-edit-btn ${
                                        editingThis ? "is-active" : ""
                                      }`}
                                      onClick={(e) => openEdit(opt, e)}
                                      title={t("modelEdit.edit")}
                                      aria-label={`${t("modelEdit.edit")} ${opt.modelId}`}
                                    >
                                      {t("modelEdit.edit")}
                                    </button>
                                  </span>
                                </li>
                              );
                            })}
                          </ul>
                        ) : null}
                      </li>
                    );
                  })}
                </ul>
              </div>

              {editing ? (
                <div
                  className="model-picker-panel model-picker-panel--edit"
                  role="dialog"
                  aria-label={t("modelEdit.title")}
                >
                  <div className="mp-edit-head">
                    <span className="mp-edit-title" title={editing.modelId}>
                      {editing.modelId}
                    </span>
                    <button
                      type="button"
                      className="mp-edit-close"
                      onClick={() => setEditing(null)}
                      aria-label={t("modelEdit.close")}
                    >
                      ×
                    </button>
                  </div>

                  {editSupportsReasoning ? (
                    <div className="mp-edit-section">
                      <ToggleSwitch
                        label={t("modelEdit.thinking")}
                        checked={editPrefs.thinking}
                        disabled={editThinkingMandatory}
                        onChange={(v) => patchEdit({ thinking: v })}
                      />
                    </div>
                  ) : null}

                  {editContexts.length > 0 ? (
                    <div className="mp-edit-section">
                      <div className="mp-edit-section-label">
                        {t("modelEdit.context")}
                      </div>
                      {editContexts.map((c) => {
                        const selected = editPrefs.context === c;
                        return (
                          <button
                            key={c}
                            type="button"
                            className={`mp-edit-choice ${selected ? "is-selected" : ""}`}
                            onClick={() =>
                              patchEdit({
                                context: selected ? "default" : c,
                              })
                            }
                          >
                            <span>{contextLabel(c)}</span>
                            {selected ? <IconCheck /> : null}
                          </button>
                        );
                      })}
                    </div>
                  ) : null}

                  {editEfforts.length > 0 ? (
                    <div className="mp-edit-section">
                      <div className="mp-edit-section-label">
                        {t("modelEdit.effort")}
                      </div>
                      {editEfforts.map((e) => {
                        const selected = editPrefs.effort === e;
                        return (
                          <button
                            key={e}
                            type="button"
                            className={`mp-edit-choice ${selected ? "is-selected" : ""}`}
                            disabled={!editPrefs.thinking}
                            onClick={() => patchEdit({ effort: e })}
                          >
                            <span>{effortLabel(e)}</span>
                            {selected ? <IconCheck /> : null}
                          </button>
                        );
                      })}
                    </div>
                  ) : null}

                  {!editSupportsReasoning &&
                  editContexts.length === 0 &&
                  editEfforts.length === 0 ? (
                    <p className="mp-edit-empty">{t("modelEdit.noOptions")}</p>
                  ) : null}
                </div>
              ) : null}
            </div>,
            document.body,
          )
        : null}
    </div>
  );
}
