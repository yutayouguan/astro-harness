/** 模型选择器。 */
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
} from "react";
import { Check, ChevronDown } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "../i18n/LocaleContext";
import {
  DEFAULT_MODEL_PREFS,
  loadAllModelPrefs,
  loadModelPrefs,
  loadPickerGlobals,
  modelPrefsKey,
  savePickerGlobals,
  upsertModelPrefs,
  type ModelContextSize,
  type ModelEffort,
  type ModelPickerGlobals,
  type ModelRuntimePrefs,
} from "../lib/modelPrefs";
import type { ModelInfo, ProviderDto, ProviderModelsResult } from "../types";
import { ModelBrandIcon } from "./ProviderIcons";

/** 模型选择器入参 */
type Props = {
  providers: ProviderDto[];
  /** 当前活跃提供商 id */
  value: string | null;
  /** 切换提供商与模型 */
  onChange: (providerId: string, model: string) => void;
  disabled?: boolean;
  /** 当前模型配置变更（便于 App 同步发送参数） */
  onActivePrefsChange?: (prefs: ModelRuntimePrefs, globals: ModelPickerGlobals) => void;
};

/** 下拉中的一项：某提供商下的模型 */
type ModelOption = {
  providerId: string;
  providerName: string;
  modelId: string;
};

/** 选中勾选图标 */
function IconCheck(props: { className?: string }) {
  return <Check size={14} strokeWidth={2.5} aria-hidden {...props} />;
}

/** 下拉箭头图标 */
function IconChevron(props: { className?: string }) {
  return <ChevronDown size={12} strokeWidth={2.5} aria-hidden {...props} />;
}

/** 确保默认模型 id 出现在列表中（缺失则插入占位项） */
function ensureDefaultModel(models: ModelInfo[], defaultId: string): ModelInfo[] {
  const id = defaultId.trim();
  if (!id) return models;
  if (models.some((m) => m.id === id)) return models;
  return [
    {
      id,
      capabilities: {
        vision: false,
        web: false,
        reasoning: false,
        tools: true,
      },
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

const EFFORTS: ModelEffort[] = ["low", "medium", "high", "xhigh", "max"];
const CONTEXTS: ModelContextSize[] = ["300k", "1m"];

/** 聊天顶栏：列出各已启用提供商的全部可用模型（缓存），选中即切换提供商并设为默认模型 */
export default function ModelPicker({
  providers,
  value,
  onChange,
  disabled = false,
  onActivePrefsChange,
}: Props) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const [options, setOptions] = useState<ModelOption[]>([]);
  const [loading, setLoading] = useState(false);
  const [editing, setEditing] = useState<ModelOption | null>(null);
  const [editPrefs, setEditPrefs] = useState<ModelRuntimePrefs>({
    ...DEFAULT_MODEL_PREFS,
  });
  const [globals, setGlobals] = useState<ModelPickerGlobals>(() =>
    loadPickerGlobals(),
  );
  const [, setPrefsTick] = useState(0);
  const ref = useRef<HTMLDivElement | null>(null);
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

  const loadOptions = useCallback(async (refreshEmpty = false) => {
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
          return models.map((m) => ({
            providerId: p.id,
            providerName: p.display_name,
            modelId: m.id,
          }));
        }),
      );
      setOptions(groups.flat());
    } finally {
      setLoading(false);
    }
  }, [providers]);

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
      if (!ref.current?.contains(e.target as Node)) {
        setOpen(false);
        setEditing(null);
      }
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [open]);

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

  const openEdit = (opt: ModelOption, e: ReactMouseEvent<HTMLButtonElement>) => {
    e.stopPropagation();
    e.preventDefault();
    const prefs = loadModelPrefs(opt.providerId, opt.modelId);
    setEditPrefs(prefs);
    setEditing(opt);
  };

  const patchEdit = (patch: Partial<ModelRuntimePrefs>) => {
    if (!editing) return;
    const next = upsertModelPrefs(editing.providerId, editing.modelId, patch);
    setEditPrefs(next);
    setPrefsTick((n) => n + 1);
    if (
      editing.providerId === activeProvider?.id &&
      editing.modelId === activeModelId
    ) {
      onActivePrefsChange?.(next, globals);
    }
  };

  const setGlobal = (patch: Partial<ModelPickerGlobals>) => {
    const next = { ...globals, ...patch };
    setGlobals(next);
    savePickerGlobals(next);
    if (next.auto) setEditing(null);
    notifyActive(next);
  };

  const effortLabel = (e: ModelEffort) => {
    switch (e) {
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
        {globals.auto ? (
          <>
            <span className="model-picker-icon is-auto" aria-hidden>
              <ModelBrandIcon modelId={activeModelId || "auto"} />
            </span>
            <span className="model-picker-label" title={t("modelEdit.auto")}>
              {t("modelEdit.auto")}
            </span>
          </>
        ) : activeProvider ? (
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
        <IconChevron className="model-picker-chevron" />
      </button>

      {open && (
        <div
          className={`model-picker-flyout ${editing && !globals.auto ? "has-edit" : ""}`}
        >
          <div className="model-picker-panel model-picker-panel--list">
            <div className="model-picker-globals">
              <ToggleSwitch
                label={t("modelEdit.auto")}
                checked={globals.auto}
                onChange={(v) => {
                  setGlobal({ auto: v });
                  if (v) setEditing(null);
                }}
              />
              <ToggleSwitch
                label={t("modelEdit.maxMode")}
                checked={globals.maxMode}
                onChange={(v) => setGlobal({ maxMode: v })}
              />
            </div>

            {globals.auto ? (
              <div className="model-picker-auto-hint" role="status">
                {t("modelEdit.autoHint")}
              </div>
            ) : (
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
              {grouped.map((group) => (
                <li
                  key={group.providerId}
                  className="model-picker-group"
                  role="presentation"
                >
                  <div className="model-picker-group-label">{group.name}</div>
                  <ul className="model-picker-group-list" role="group">
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
                          : prefs?.effort === "max" || prefs?.effort === "xhigh"
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
                              if (!selected) onChange(opt.providerId, opt.modelId);
                              setOpen(false);
                              setEditing(null);
                            }}
                          >
                            <span className="model-picker-option-icon" aria-hidden>
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
                              <span className="model-picker-option-provider">
                                {opt.providerName}
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
                </li>
              ))}
              </ul>
            )}
          </div>

          {editing && !globals.auto ? (
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

              <div className="mp-edit-section">
                <ToggleSwitch
                  label={t("modelEdit.thinking")}
                  checked={editPrefs.thinking}
                  onChange={(v) => patchEdit({ thinking: v })}
                />
                <ToggleSwitch
                  label={t("modelEdit.fast")}
                  checked={editPrefs.fast}
                  onChange={(v) => patchEdit({ fast: v })}
                />
              </div>

              <div className="mp-edit-section">
                <div className="mp-edit-section-label">
                  {t("modelEdit.context")}
                </div>
                {CONTEXTS.map((c) => {
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
                      <span>{c === "300k" ? "300K" : "1M"}</span>
                      {selected ? <IconCheck /> : null}
                    </button>
                  );
                })}
              </div>

              <div className="mp-edit-section">
                <div className="mp-edit-section-label">
                  {t("modelEdit.effort")}
                </div>
                {EFFORTS.map((e) => {
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
            </div>
          ) : null}
        </div>
      )}
    </div>
  );
}
