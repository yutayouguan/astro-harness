/** 新建/编辑定时任务：右侧抽屉。 */
import { useCallback, useEffect, useState, type CSSProperties } from "react";
import {
  CalendarPlus,
  Check,
  Cpu,
  MessageSquareText,
  MessagesSquare,
  Pencil,
  Save,
  Server,
  SlidersHorizontal,
  Type,
  X,
} from "lucide-react";
import {
  ChevronDown as ChevronDownData,
  ChevronUp as ChevronUpData,
} from "lucide";
import { invoke } from "@tauri-apps/api/core";
import { ScheduleEditor } from "./ScheduleEditor";
import { MorphToggleIcon } from "../icons/MorphIcon";
import { Drawer, SelectMenu } from "../ui";
import { ModelBrandIcon, ProviderBrandIcon } from "../icons/ProviderIcons";
import {
  decodeSchedule,
  encodeSchedule,
  type ScheduleDraft,
} from "../../lib/cron/cronSchedule";
import { useI18n } from "../../i18n/LocaleContext";
import type { CronJobDto } from "./CronPanel";

/** 创建任务对话框可选的供应商简项 */
export type ProviderOpt = {
  id: string;
  name: string;
  model: string;
  /** 供应商品牌 kind，用于 lobehub 图标 */
  kind?: string;
};

/** 新建/编辑定时任务抽屉入参 */
/** 模板预填值（仅预填表单，不进入编辑模式） */
export type CronPrefill = {
  title: string;
  task: string;
  schedule: string;
};

type Props = {
  open: boolean;
  onClose: () => void;
  /** 创建或保存成功后刷新列表 */
  onCreated: () => void;
  providers: ProviderOpt[];
  activeProviderId: string | null;
  models?: string[];
  /** 传入则进入编辑模式 */
  editingJob?: CronJobDto | null;
  /** 模板预填（仅填入表单，提交时走创建流程） */
  prefill?: CronPrefill | null;
  toneStyle?: CSSProperties;
};

const DEFAULT_DRAFT: ScheduleDraft = {
  mode: "daily",
  time: "09:00",
  weekdays: [],
};

/** 优先用当前激活供应商，否则取列表首项 */
function defaultProviderId(
  providers: ProviderOpt[],
  activeProviderId: string | null,
): string {
  if (activeProviderId && providers.some((p) => p.id === activeProviderId)) {
    return activeProviderId;
  }
  return providers[0]?.id ?? "";
}

/** 新建或编辑定时任务（右侧抽屉） */
export function CreateCronDialog({
  open,
  onClose,
  onCreated,
  providers,
  activeProviderId,
  models: modelsProp,
  editingJob = null,
  prefill = null,
  toneStyle,
}: Props) {
  const { t } = useI18n();
  const isEdit = Boolean(editingJob);

  const [title, setTitle] = useState("");
  const [task, setTask] = useState("");
  const [draft, setDraft] = useState<ScheduleDraft>(DEFAULT_DRAFT);
  const [selectedProviderId, setSelectedProviderId] = useState(() =>
    defaultProviderId(providers, activeProviderId),
  );
  const [modelOptions, setModelOptions] = useState<string[]>([]);
  const [selectedModel, setSelectedModel] = useState("");
  const [showInChat, setShowInChat] = useState(false);
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [error, setError] = useState("");
  const [saving, setSaving] = useState(false);

  const resetForm = useCallback(() => {
    setTitle("");
    setTask("");
    setDraft({ ...DEFAULT_DRAFT, weekdays: [] });
    setSelectedProviderId(defaultProviderId(providers, activeProviderId));
    setModelOptions([]);
    setSelectedModel("");
    setShowInChat(false);
    setError("");
    setSaving(false);
  }, [providers, activeProviderId]);

  useEffect(() => {
    if (!open) return;
    if (editingJob) {
      setTitle(editingJob.title);
      setTask(editingJob.task);
      setDraft(decodeSchedule(editingJob.schedule));
      setSelectedProviderId(
        editingJob.provider_id &&
          providers.some((p) => p.id === editingJob.provider_id)
          ? editingJob.provider_id
          : defaultProviderId(providers, activeProviderId),
      );
      setSelectedModel(editingJob.model ?? "");
      setShowInChat(Boolean(editingJob.show_in_chat));
      setError("");
      setSaving(false);
      return;
    }
    if (prefill) {
      setTitle(prefill.title);
      setTask(prefill.task);
      setDraft(decodeSchedule(prefill.schedule));
    }
    setSelectedProviderId(defaultProviderId(providers, activeProviderId));
    setShowInChat(false);
    setError("");
  }, [open, providers, activeProviderId, editingJob, prefill]);

  useEffect(() => {
    if (!open) return;

    const provider =
      providers.find((p) => p.id === selectedProviderId) ??
      providers.find((p) => p.id === activeProviderId) ??
      providers[0];
    const providerId = provider?.id ?? selectedProviderId;
    const fallbackModel = provider?.model?.trim() ?? "";

    let cancelled = false;

    void (async () => {
      let options: string[] = [];

      if (modelsProp && modelsProp.length > 0) {
        options = [...modelsProp];
      } else if (providerId) {
        try {
          const cached = await invoke<{
            models: { id: string }[] | string[];
          } | null>("get_cached_provider_models", { id: providerId });
          if (cached?.models?.length) {
            options = cached.models.map((m) =>
              typeof m === "string" ? m : m.id,
            );
          }
        } catch {
          // ignore
        }
      }

      if (options.length === 0 && fallbackModel) {
        options = [fallbackModel];
      }
      if (editingJob?.model && !options.includes(editingJob.model)) {
        options = [editingJob.model, ...options];
      }

      if (cancelled) return;
      setModelOptions(options);
      setSelectedModel((prev) =>
        prev && options.includes(prev) ? prev : options[0] ?? "",
      );
    })();

    return () => {
      cancelled = true;
    };
  }, [open, modelsProp, selectedProviderId, providers, activeProviderId, editingJob]);

  async function handleSave() {
    setError("");
    if (!title.trim() || !task.trim()) {
      setError(t("cron.dialog.error"));
      return;
    }
    if (draft.mode === "once" && !draft.onceAt?.trim()) {
      setError(t("cron.dialog.onceRequired"));
      return;
    }

    setSaving(true);
    try {
      const payload = {
        schedule: encodeSchedule(draft),
        task: task.trim(),
        title: title.trim(),
        agent_id: "default",
        provider_id: selectedProviderId || null,
        model: selectedModel || null,
        show_in_chat: showInChat,
      };
      if (editingJob) {
        await invoke("update_cron_job", {
          args: { id: editingJob.id, ...payload },
        });
      } else {
        await invoke("add_cron_job", { args: payload });
      }
      onCreated();
      onClose();
      if (!editingJob) resetForm();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  }

  const TitleIcon = isEdit ? Pencil : CalendarPlus;

  return (
    <Drawer
      open={open}
      onClose={onClose}
      size="lg"
      backdropClassName="cron-create-drawer-backdrop"
      backdropStyle={toneStyle}
      className="cron-create-drawer"
      aria-labelledby="cron-create-drawer-title"
    >
        <header className="cron-create-drawer-head">
          <h2 id="cron-create-drawer-title" className="cron-create-drawer-title">
            <span className="cron-create-drawer-title-icon" aria-hidden>
              <TitleIcon size={16} strokeWidth={2.3} />
            </span>
            {isEdit ? t("cron.dialog.editTitle") : t("cron.dialog.title")}
          </h2>
          <button
            type="button"
            className="cron-dialog-close"
            onClick={onClose}
            aria-label={t("cron.cancel")}
          >
            <X size={16} strokeWidth={2.5} aria-hidden />
          </button>
        </header>

        <div className="cron-create-drawer-scroll">
          <div className="cron-dialog-row">
            <label className="cron-dialog-field">
              <span className="cron-dialog-label">
                <Type size={13} strokeWidth={2.2} aria-hidden />
                {t("cron.field.name")}
                <span className="cron-dialog-req" aria-hidden>
                  *
                </span>
              </span>
              <input
                type="text"
                className="cron-dialog-input"
                value={title}
                onChange={(e) => setTitle(e.target.value)}
                placeholder={t("cron.field.namePlaceholder")}
                autoFocus
              />
            </label>
          </div>

          <label className="cron-dialog-field">
            <span className="cron-dialog-label">
              <MessageSquareText size={13} strokeWidth={2.2} aria-hidden />
              {t("cron.field.task")}
              <span className="cron-dialog-req" aria-hidden>
                *
              </span>
            </span>
            <textarea
              className="cron-dialog-textarea"
              value={task}
              onChange={(e) => setTask(e.target.value)}
              placeholder={t("cron.field.taskPlaceholder")}
              rows={4}
            />
          </label>

          <ScheduleEditor value={draft} onChange={setDraft} />

          <details
            className="cron-dialog-advanced"
            onToggle={(e) => setAdvancedOpen(e.currentTarget.open)}
          >
            <summary>
              <span className="cron-dialog-advanced-label">
                <SlidersHorizontal size={14} strokeWidth={2.2} aria-hidden />
                {t("cron.advanced")}
              </span>
              <MorphToggleIcon
                active={advancedOpen}
                activeIcon={ChevronUpData}
                inactiveIcon={ChevronDownData}
                className="cron-dialog-advanced-morph"
                size={14}
                strokeWidth={2.2}
                aria-hidden
              />
            </summary>
            <div className="cron-dialog-advanced-body">
              {providers.length > 1 && (
                <label className="cron-dialog-field">
                  <span className="cron-dialog-label">
                    <Server size={13} strokeWidth={2.2} aria-hidden />
                    {t("cron.field.provider")}
                  </span>
                  <SelectMenu
                    className="cron-dialog-select cron-dialog-select--brand"
                    value={selectedProviderId}
                    onChange={setSelectedProviderId}
                    aria-label={t("cron.field.provider")}
                      options={providers.map((p) => ({
                        value: p.id,
                        label: p.name,
                        icon: (
                          <span className="select-menu-brand-icon" aria-hidden>
                            <ProviderBrandIcon kind={p.kind ?? p.id} size={13} />
                          </span>
                        ),
                      }))}
                  />
                </label>
              )}
              <label className="cron-dialog-field">
                <span className="cron-dialog-label">
                  <Cpu size={13} strokeWidth={2.2} aria-hidden />
                  {t("cron.field.model")}
                </span>
                <SelectMenu
                  className="cron-dialog-select cron-dialog-select--brand"
                  value={selectedModel}
                  onChange={setSelectedModel}
                  aria-label={t("cron.field.model")}
                  disabled={modelOptions.length === 0}
                  options={
                    modelOptions.length === 0
                      ? [{ value: "", label: "—" }]
                      : modelOptions.map((m) => ({
                          value: m,
                          label: m,
                          icon: <ModelBrandIcon modelId={m} size={13} />,
                        }))
                  }
                />
              </label>
              <label
                className={`cron-dialog-check${showInChat ? " is-checked" : ""}`}
              >
                <input
                  type="checkbox"
                  className="cron-dialog-check-input"
                  checked={showInChat}
                  onChange={(e) => setShowInChat(e.target.checked)}
                />
                <span className="cron-dialog-check-box" aria-hidden>
                  {showInChat ? <Check size={12} strokeWidth={2.8} /> : null}
                </span>
                <span className="cron-dialog-check-copy">
                  <strong>
                    <MessagesSquare size={13} strokeWidth={2.2} aria-hidden />
                    {t("cron.field.showInChat")}
                  </strong>
                  <small>{t("cron.field.showInChatHint")}</small>
                </span>
              </label>
            </div>
          </details>

          {error && <p className="cron-dialog-error">{error}</p>}
        </div>

        <footer className="cron-create-drawer-foot">
          <button type="button" className="cron-btn-ghost" onClick={onClose}>
            <X size={14} strokeWidth={2.3} aria-hidden />
            {t("cron.cancel")}
          </button>
          <button
            type="button"
            className="cron-btn-primary"
            onClick={() => void handleSave()}
            disabled={saving}
          >
            <Save size={14} strokeWidth={2.3} aria-hidden />
            {t("cron.save")}
          </button>
        </footer>
    </Drawer>
  );
}
