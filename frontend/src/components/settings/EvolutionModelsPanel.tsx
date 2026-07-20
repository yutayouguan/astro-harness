import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type CSSProperties,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  AlertTriangle,
  Beaker,
  Check,
  ClipboardList,
  Dna,
  FilePlus2,
  FlaskConical,
  Gavel,
  GitBranch,
  History,
  Inbox,
  Pencil,
  Play,
  Power,
  RefreshCw,
  Settings2,
  ShieldCheck,
  Sparkles,
  Timer,
  TrendingUp,
  Trash2,
  type LucideIcon,
} from "lucide-react";
import { useEvolutionSettings } from "../../hooks/settings/useEvolutionSettings";
import { useEvolutionProposals } from "../../hooks/settings/useEvolutionProposals";
import { useEvalExamples } from "../../hooks/settings/useEvalExamples";
import { useDspy } from "../../hooks/settings/useDspy";
import { useEvolutionHistory } from "../../hooks/settings/useEvolutionHistory";
import { useEvolutionAuto } from "../../hooks/settings/useEvolutionAuto";
import { useSkillCurator } from "../../hooks/settings/useSkillCurator";
import { useI18n } from "../../i18n/LocaleContext";
import type { MessageKey } from "../../i18n/messages";
import type {
  EvolutionRouteDto,
  EvolutionRouteId,
  InstalledSkill,
  ModelInfo,
  ProviderDto,
  ProviderModelsResult,
  ProvidersStateDto,
} from "../../types";
import { SelectMenu } from "../ui/SelectMenu";

type Props = {
  active: boolean;
};

type EvoSection = "setup" | "run" | "history" | "lab";

const SECTIONS: {
  id: EvoSection;
  labelKey: MessageKey;
  Icon: LucideIcon;
}[] = [
  { id: "setup", labelKey: "evo.section.setup", Icon: Settings2 },
  { id: "run", labelKey: "evo.section.run", Icon: Play },
  { id: "history", labelKey: "evo.section.history", Icon: History },
  { id: "lab", labelKey: "evo.section.lab", Icon: FlaskConical },
];

const ROUTES: {
  id: EvolutionRouteId;
  labelKey: MessageKey;
  descKey: MessageKey;
  Icon: LucideIcon;
}[] = [
  {
    id: "reflection",
    labelKey: "evo.reflection",
    descKey: "evo.reflectionDesc",
    Icon: Sparkles,
  },
  {
    id: "judge",
    labelKey: "evo.judge",
    descKey: "evo.judgeDesc",
    Icon: Gavel,
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
            image_gen: false,
            video_gen: false,
            audio_gen: false,
            music_gen: false,
          },
        },
        ...models,
      ];
}

export default function EvolutionModelsPanel({ active }: Props) {
  const { t } = useI18n();
  const {
    loading,
    error,
    settings,
    setEnabled,
    setRoute,
    resetRoute,
    setGates,
    setSearch,
    setAuto,
    setCurator,
    reload,
  } = useEvolutionSettings(active);
  const {
    running,
    runMode,
    error: evoError,
    lastReport,
    lastSearch,
    proposals,
    run: runEvolution,
    runSearch,
    cancelSearch,
    approve,
    approveToBranch,
    reject,
    reload: reloadProposals,
  } = useEvolutionProposals(active);
  const [branchMsg, setBranchMsg] = useState<string | null>(null);
  const [genDraft, setGenDraft] = useState("");
  const [varDraft, setVarDraft] = useState("");
  const [popDraft, setPopDraft] = useState("");
  const [evalExDraft, setEvalExDraft] = useState("");
  const [llmDraft, setLlmDraft] = useState("");
  const {
    examples: evalExamples,
    importCandidates,
    importLoading,
    add: addEval,
    remove: removeEval,
    reloadImportCandidates,
    importFromSession,
    error: evalError,
  } = useEvalExamples(active);
  const [evalTask, setEvalTask] = useState("");
  const [evalSkill, setEvalSkill] = useState("");
  const [evalExpect, setEvalExpect] = useState("");
  const [evalVerdict, setEvalVerdict] = useState<"fail" | "pass">("fail");
  const [searchFocusSkill, setSearchFocusSkill] = useState("");
  const {
    status: dspyStatus,
    busy: dspyBusy,
    error: dspyError,
    message: dspyMessage,
    setup: setupDspy,
    run: runDspy,
  } = useDspy(active);
  const {
    loading: curatorLoading,
    error: curatorError,
    report: curatorReport,
    status: curatorStatus,
    lastEnqueued,
    autoNotice: curatorAutoNotice,
    clearAutoNotice: clearCuratorAutoNotice,
    run: runCurator,
    enqueue: enqueueCurator,
    reload: reloadCurator,
  } = useSkillCurator(active);
  const [curatorEnqueue, setCuratorEnqueue] = useState(false);
  const [dspySkill, setDspySkill] = useState("");
  const { history, reload: reloadHistory } = useEvolutionHistory(active);
  const {
    status: autoStatus,
    error: autoStatusError,
    reload: reloadAutoStatus,
  } = useEvolutionAuto(active);
  const [cooldownDraft, setCooldownDraft] = useState("");
  const [minDecDraft, setMinDecDraft] = useState("");
  const [maxRunsDraft, setMaxRunsDraft] = useState("");
  const [curatorIntervalDraft, setCuratorIntervalDraft] = useState("");
  const [curatorMaxEnqueueDraft, setCuratorMaxEnqueueDraft] = useState("");
  const [curatorMaxLlmDraft, setCuratorMaxLlmDraft] = useState("");
  const [section, setSection] = useState<EvoSection>("setup");
  const [expandedProposals, setExpandedProposals] = useState<Set<string>>(() => new Set());
  const [installedSkills, setInstalledSkills] = useState<InstalledSkill[]>([]);

  const submitEval = () => {
    if (!evalTask.trim()) return;
    void addEval({
      skillId: evalSkill.trim() || null,
      task: evalTask.trim(),
      expectations: evalExpect
        .split("\n")
        .map((s) => s.trim())
        .filter(Boolean),
      verdict: evalVerdict,
    }).then(() => {
      setEvalTask("");
      setEvalSkill("");
      setEvalExpect("");
    });
  };
  const [providersState, setProvidersState] = useState<ProvidersStateDto | null>(null);
  const [providersError, setProvidersError] = useState<string | null>(null);
  const [editingRoute, setEditingRoute] = useState<EvolutionRouteId | null>(null);
  const [selectedProviderId, setSelectedProviderId] = useState("");
  const [modelsByProvider, setModelsByProvider] = useState<Record<string, ModelInfo[]>>({});
  const [loadingModelsFor, setLoadingModelsFor] = useState<string | null>(null);
  const [maxBytesDraft, setMaxBytesDraft] = useState("");
  const [judgeDraft, setJudgeDraft] = useState("");

  useEffect(() => {
    if (!active || section !== "lab") return;
    void reloadImportCandidates();
  }, [active, section, reloadImportCandidates]);

  useEffect(() => {
    if (!active || (section !== "lab" && section !== "run")) return;
    void invoke<InstalledSkill[]>("list_installed_skills", { agentId: null, scope: null })
      .then(setInstalledSkills)
      .catch(() => setInstalledSkills([]));
  }, [active, section]);

  const evalSkillOptions = useMemo(
    () => [
      { value: "", label: t("evo.evalSkillGeneric") },
      ...installedSkills
        .filter((s) => s.enabled)
        .map((s) => ({ value: s.id, label: s.name ? `${s.name} · ${s.id}` : s.id })),
    ],
    [installedSkills, t],
  );

  const searchFocusOptions = useMemo(
    () => [
      { value: "", label: t("evo.focusSkillAny") },
      ...installedSkills
        .filter((s) => s.enabled)
        .map((s) => ({ value: s.id, label: s.name ? `${s.name} · ${s.id}` : s.id })),
    ],
    [installedSkills, t],
  );

  const enabledProviders = useMemo(
    () => (providersState?.providers ?? []).filter((p) => p.enabled),
    [providersState],
  );

  const routeRows = useMemo(() => {
    const byId = new Map((settings?.routes ?? []).map((r) => [r.id, r]));
    return ROUTES.map((r) => byId.get(r.id)).filter(Boolean) as EvolutionRouteDto[];
  }, [settings]);

  useEffect(() => {
    if (settings) {
      setMaxBytesDraft(String(settings.gates.maxSkillBytes));
      setJudgeDraft(String(settings.gates.minJudgeScore));
      setGenDraft(String(settings.search.generations));
      setVarDraft(String(settings.search.variants));
      setPopDraft(String(settings.search.populationSize));
      setEvalExDraft(String(settings.search.maxEvalExamples));
      setLlmDraft(String(settings.search.maxLlmCalls));
      setCooldownDraft(String(settings.auto.cooldownSecs));
      setMinDecDraft(String(settings.auto.minNewDecisions));
      setMaxRunsDraft(String(settings.auto.maxRunsPerDay));
      setCuratorIntervalDraft(String(settings.curator.intervalDays));
      setCuratorMaxEnqueueDraft(String(settings.curator.maxEnqueue));
      setCuratorMaxLlmDraft(String(settings.curator.maxLlmCalls));
    }
  }, [settings]);

  const commitAutoNums = useCallback(() => {
    if (!settings) return;
    const cd = Number.parseInt(cooldownDraft, 10);
    const md = Number.parseInt(minDecDraft, 10);
    const mr = Number.parseInt(maxRunsDraft, 10);
    const cooldown =
      Number.isFinite(cd) && cd >= 60 && cd <= 86400 ? cd : settings.auto.cooldownSecs;
    const minDec =
      Number.isFinite(md) && md >= 1 && md <= 50 ? md : settings.auto.minNewDecisions;
    const maxRuns =
      Number.isFinite(mr) && mr >= 1 && mr <= 24 ? mr : settings.auto.maxRunsPerDay;
    if (
      cooldown !== settings.auto.cooldownSecs ||
      minDec !== settings.auto.minNewDecisions ||
      maxRuns !== settings.auto.maxRunsPerDay
    ) {
      void setAuto(settings.auto.enabled, cooldown, minDec, maxRuns).then(() =>
        reloadAutoStatus(),
      );
    } else {
      setCooldownDraft(String(settings.auto.cooldownSecs));
      setMinDecDraft(String(settings.auto.minNewDecisions));
      setMaxRunsDraft(String(settings.auto.maxRunsPerDay));
    }
  }, [settings, cooldownDraft, minDecDraft, maxRunsDraft, setAuto, reloadAutoStatus]);

  const commitCuratorNums = useCallback(() => {
    if (!settings) return;
    const id = Number.parseInt(curatorIntervalDraft, 10);
    const me = Number.parseInt(curatorMaxEnqueueDraft, 10);
    const ml = Number.parseInt(curatorMaxLlmDraft, 10);
    const interval =
      Number.isFinite(id) && id >= 1 && id <= 90 ? id : settings.curator.intervalDays;
    const maxEnqueue =
      Number.isFinite(me) && me >= 1 && me <= 20 ? me : settings.curator.maxEnqueue;
    const maxLlm =
      Number.isFinite(ml) && ml >= 1 && ml <= 20 ? ml : settings.curator.maxLlmCalls;
    if (
      interval !== settings.curator.intervalDays ||
      maxEnqueue !== settings.curator.maxEnqueue ||
      maxLlm !== settings.curator.maxLlmCalls
    ) {
      void setCurator(
        settings.curator.enabled,
        interval,
        maxEnqueue,
        settings.curator.llmDiagnose,
        maxLlm,
      ).then(() => reloadCurator());
    } else {
      setCuratorIntervalDraft(String(settings.curator.intervalDays));
      setCuratorMaxEnqueueDraft(String(settings.curator.maxEnqueue));
      setCuratorMaxLlmDraft(String(settings.curator.maxLlmCalls));
    }
  }, [
    settings,
    curatorIntervalDraft,
    curatorMaxEnqueueDraft,
    curatorMaxLlmDraft,
    setCurator,
    reloadCurator,
  ]);

  const commitSearch = useCallback(() => {
    if (!settings) return;
    const g = Number.parseInt(genDraft, 10);
    const v = Number.parseInt(varDraft, 10);
    const p = Number.parseInt(popDraft, 10);
    const e = Number.parseInt(evalExDraft, 10);
    const l = Number.parseInt(llmDraft, 10);
    const gen = Number.isFinite(g) && g >= 1 && g <= 6 ? g : settings.search.generations;
    const vari = Number.isFinite(v) && v >= 1 && v <= 6 ? v : settings.search.variants;
    const pop = Number.isFinite(p) && p >= 1 && p <= 8 ? p : settings.search.populationSize;
    const evalEx =
      Number.isFinite(e) && e >= 0 && e <= 32 ? e : settings.search.maxEvalExamples;
    const llm = Number.isFinite(l) && l >= 0 && l <= 500 ? l : settings.search.maxLlmCalls;
    if (
      gen !== settings.search.generations ||
      vari !== settings.search.variants ||
      pop !== settings.search.populationSize ||
      evalEx !== settings.search.maxEvalExamples ||
      llm !== settings.search.maxLlmCalls
    ) {
      void setSearch(gen, vari, settings.search.crossover, pop, evalEx, llm);
    } else {
      setGenDraft(String(settings.search.generations));
      setVarDraft(String(settings.search.variants));
      setPopDraft(String(settings.search.populationSize));
      setEvalExDraft(String(settings.search.maxEvalExamples));
      setLlmDraft(String(settings.search.maxLlmCalls));
    }
  }, [settings, genDraft, varDraft, popDraft, evalExDraft, llmDraft, setSearch]);

  const toggleCrossover = useCallback(() => {
    if (!settings) return;
    void setSearch(
      settings.search.generations,
      settings.search.variants,
      !settings.search.crossover,
      settings.search.populationSize,
      settings.search.maxEvalExamples,
      settings.search.maxLlmCalls,
    );
  }, [settings, setSearch]);

  const providerOptions = useMemo(
    () => enabledProviders.map((p) => ({ value: p.id, label: p.display_name })),
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
    setProvidersError(null);
    try {
      const next = await invoke<ProvidersStateDto>("get_providers_state");
      setProvidersState(next);
    } catch (err) {
      setProvidersError(err instanceof Error ? err.message : String(err));
    }
  }, [active]);

  useEffect(() => {
    void loadProviders();
  }, [loadProviders]);

  const loadModels = useCallback(async (provider: ProviderDto) => {
    setLoadingModelsFor(provider.id);
    try {
      let models: ModelInfo[] = [];
      try {
        const cached = await invoke<ProviderModelsResult | null>("get_cached_provider_models", {
          id: provider.id,
        });
        if (cached?.models?.length) models = cached.models;
      } catch {
        // cache miss expected for new providers
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
  }, []);

  const beginEdit = useCallback(
    (row: EvolutionRouteDto) => {
      const nextProviderId =
        row.provider !== "auto"
          ? row.provider
          : providersState?.active_provider_id ?? enabledProviders[0]?.id ?? "";
      setEditingRoute(row.id);
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
      if (!editingRoute || !selectedProviderId) return;
      await setRoute(editingRoute, selectedProviderId, model);
      setEditingRoute(null);
    },
    [editingRoute, selectedProviderId, setRoute],
  );

  const gates = settings?.gates;

  const commitMaxBytes = useCallback(() => {
    if (!gates) return;
    const parsed = Number.parseInt(maxBytesDraft, 10);
    const next = Number.isFinite(parsed) && parsed > 0 ? parsed : gates.maxSkillBytes;
    if (next !== gates.maxSkillBytes) {
      void setGates(gates.runTests, next, gates.requirePr, gates.minJudgeScore);
    } else {
      setMaxBytesDraft(String(gates.maxSkillBytes));
    }
  }, [gates, maxBytesDraft, setGates]);

  const commitJudge = useCallback(() => {
    if (!gates) return;
    const parsed = Number.parseFloat(judgeDraft);
    const next =
      Number.isFinite(parsed) && parsed >= 0 && parsed <= 1 ? parsed : gates.minJudgeScore;
    if (Math.abs(next - gates.minJudgeScore) > 1e-6) {
      void setGates(gates.runTests, gates.maxSkillBytes, gates.requirePr, next);
    } else {
      setJudgeDraft(String(gates.minJudgeScore));
    }
  }, [gates, judgeDraft, setGates]);

  const evoOn = settings?.enabled ?? false;
  const pendingCount = proposals.length;
  const adoptionPct = history ? Math.round(history.summary.adoptionRate * 100) : null;

  return (
    <div className="evo-page aux-page aux-page-embedded" data-tone="blue">
      <header className="evo-toolbar">
        <div className="evo-toolbar-main">
          <div className="evo-brand">
            <span className="evo-brand-mark" aria-hidden>
              <Dna size={18} strokeWidth={2.2} />
            </span>
            <div className="evo-brand-text">
              <h2>{t("evo.title")}</h2>
              <p>{t("evo.subtitle")}</p>
            </div>
          </div>

          <div className="evo-status-strip" aria-live="polite">
            <span className={`evo-status-chip${evoOn ? " is-on" : ""}`}>
              <Power size={12} strokeWidth={2.4} aria-hidden />
              {evoOn ? t("evo.statusOn") : t("evo.statusOff")}
            </span>
            {pendingCount > 0 && (
              <button
                type="button"
                className="evo-status-chip evo-status-chip-btn is-pending"
                onClick={() => setSection("run")}
              >
                <Inbox size={12} strokeWidth={2.3} aria-hidden />
                {t("evo.pendingCount").replace("{n}", String(pendingCount))}
              </button>
            )}
            {adoptionPct != null && (
              <span className="evo-status-chip is-muted">
                <TrendingUp size={12} strokeWidth={2.3} aria-hidden />
                {t("evo.statAdoption")} {adoptionPct}%
              </span>
            )}
            <button
              type="button"
              role="switch"
              className="tool-toggle evo-master-toggle"
              aria-checked={evoOn}
              aria-label={t("evo.enabled")}
              onClick={() => void setEnabled(!evoOn)}
              disabled={loading || !settings}
            >
              <span className="tool-toggle-thumb" />
            </button>
          </div>
        </div>

        <div className="evo-toolbar-row">
          <div className="evo-seg" role="tablist" aria-label={t("evo.sections")}>
            {SECTIONS.map(({ id, labelKey, Icon }) => (
              <button
                key={id}
                type="button"
                role="tab"
                aria-selected={section === id}
                className={`evo-seg-item${section === id ? " is-active" : ""}`}
                onClick={() => setSection(id)}
              >
                <Icon size={14} strokeWidth={2.25} aria-hidden />
                {t(labelKey)}
                {id === "run" && pendingCount > 0 ? (
                  <span className="evo-seg-badge">{pendingCount}</span>
                ) : null}
              </button>
            ))}
          </div>
          <button
            type="button"
            className="aux-action aux-action-ghost evo-refresh-btn"
            onClick={() => {
              void reload();
              void reloadProposals();
              void reloadHistory();
              void reloadAutoStatus();
            }}
            disabled={loading}
          >
            <RefreshCw size={15} />
            {t("aux.refresh")}
          </button>
        </div>
      </header>

      {(error || providersError) && (
        <div className="aux-error">
          <AlertTriangle size={16} />
          {error ?? providersError}
        </div>
      )}

      {section === "setup" && (
        <>
          <section className="prefs-card aux-list-card evo-card">
            <div className="aux-list-head">
              <div>
                <h2 className="prefs-card-title evo-card-title">
                  <span className="evo-card-title-icon" aria-hidden>
                    <Dna size={15} />
                  </span>
                  {t("evo.enabled")}
                </h2>
                <p className="prefs-card-sub">{t("evo.enabledDesc")}</p>
              </div>
            </div>

            <div className="aux-task-list">
              {ROUTES.map((route) => {
                const row = routeRows.find((item) => item.id === route.id);
                const Icon = route.Icon;
                const isEditing = editingRoute === route.id;
                const isAuto = !row || row.provider === "auto";
                return (
                  <article className="aux-task-row" key={route.id}>
                    <div className="aux-task-icon">
                      <Icon size={18} />
                    </div>
                    <div className="aux-task-main">
                      <div className="aux-task-titleline">
                        <h3>{t(route.labelKey)}</h3>
                        <span className={`aux-route-pill${isAuto ? " is-auto" : ""}`}>
                          {isAuto ? t("aux.auto") : t("aux.custom")}
                        </span>
                      </div>
                      <p>{t(route.descKey)}</p>
                      <div className="aux-task-current">
                        <span>{row?.displayLabel ?? t("aux.followPrimary")}</span>
                        {row?.unavailable && (
                          <span className="aux-warning">
                            <AlertTriangle size={13} />
                            {t("aux.unavailable")}
                          </span>
                        )}
                      </div>

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
                            onClick={() => setEditingRoute(null)}
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
                          void resetRoute(route.id);
                          if (editingRoute === route.id) setEditingRoute(null);
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
                              id: route.id,
                              provider: "auto",
                              model: "auto",
                              displayLabel: t("aux.followPrimary"),
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

          <div className="evo-split">
            <section className="prefs-card aux-list-card evo-card">
              <div className="aux-list-head">
                <div>
                  <h2 className="prefs-card-title evo-card-title">
                    <span className="evo-card-title-icon" aria-hidden>
                      <ShieldCheck size={15} />
                    </span>
                    {t("evo.gatesTitle")}
                  </h2>
                  <p className="prefs-card-sub">{t("evo.gatesSub")}</p>
                </div>
              </div>

              <article className="aux-task-row evo-compact-row">
                <div className="aux-task-main">
                  <div className="aux-task-titleline">
                    <h3>{t("evo.runTests")}</h3>
                  </div>
                  <p>{t("evo.runTestsDesc")}</p>
                </div>
                <div className="aux-task-actions">
                  <button
                    type="button"
                    role="switch"
                    className="tool-toggle"
                    aria-checked={gates?.runTests ?? false}
                    aria-label={t("evo.runTests")}
                    onClick={() =>
                      gates &&
                      void setGates(
                        !gates.runTests,
                        gates.maxSkillBytes,
                        gates.requirePr,
                        gates.minJudgeScore,
                      )
                    }
                    disabled={!gates}
                  >
                    <span className="tool-toggle-thumb" />
                  </button>
                </div>
              </article>

              <article className="aux-task-row evo-compact-row">
                <div className="aux-task-main">
                  <div className="aux-task-titleline">
                    <h3>{t("evo.requirePr")}</h3>
                    <span className="aux-route-pill">{t("evo.requirePrLocked")}</span>
                  </div>
                  <p>{t("evo.requirePrDesc")}</p>
                </div>
              </article>

              <article className="aux-task-row evo-compact-row">
                <div className="aux-task-main">
                  <div className="aux-task-titleline">
                    <h3>{t("evo.maxSkillBytes")}</h3>
                  </div>
                  <p>{t("evo.maxSkillBytesDesc")}</p>
                </div>
                <div className="aux-task-actions">
                  <input
                    type="number"
                    className="aux-number-input"
                    min={1024}
                    step={1024}
                    value={maxBytesDraft}
                    onChange={(e) => setMaxBytesDraft(e.target.value)}
                    onBlur={commitMaxBytes}
                    disabled={!gates}
                    aria-label={t("evo.maxSkillBytes")}
                  />
                </div>
              </article>

              <article className="aux-task-row evo-compact-row">
                <div className="aux-task-main">
                  <div className="aux-task-titleline">
                    <h3>{t("evo.minJudgeScore")}</h3>
                  </div>
                  <p>{t("evo.minJudgeScoreDesc")}</p>
                </div>
                <div className="aux-task-actions">
                  <input
                    type="number"
                    className="aux-number-input"
                    min={0}
                    max={1}
                    step={0.05}
                    value={judgeDraft}
                    onChange={(e) => setJudgeDraft(e.target.value)}
                    onBlur={commitJudge}
                    disabled={!gates}
                    aria-label={t("evo.minJudgeScore")}
                  />
                </div>
              </article>
            </section>

            <section className="prefs-card aux-list-card evo-card">
              <div className="aux-list-head">
                <div>
                  <h2 className="prefs-card-title evo-card-title">
                    <span className="evo-card-title-icon" aria-hidden>
                      <Timer size={15} />
                    </span>
                    {t("evo.autoTitle")}
                  </h2>
                  <p className="prefs-card-sub">{t("evo.autoSub")}</p>
                </div>
              </div>

              <article className="aux-task-row evo-compact-row">
                <div className="aux-task-icon">
                  <Timer size={18} />
                </div>
                <div className="aux-task-main">
                  <div className="aux-task-titleline">
                    <h3>{t("evo.autoEnabled")}</h3>
                  </div>
                  <p>{t("evo.autoEnabledDesc")}</p>
                </div>
                <div className="aux-task-actions">
                  <button
                    type="button"
                    role="switch"
                    className="tool-toggle"
                    aria-checked={settings?.auto.enabled ?? false}
                    aria-label={t("evo.autoEnabled")}
                    onClick={() =>
                      settings &&
                      void setAuto(
                        !settings.auto.enabled,
                        settings.auto.cooldownSecs,
                        settings.auto.minNewDecisions,
                        settings.auto.maxRunsPerDay,
                      ).then(() => reloadAutoStatus())
                    }
                    disabled={loading || !settings || !settings.enabled}
                  >
                    <span className="tool-toggle-thumb" />
                  </button>
                </div>
              </article>

              {!settings?.enabled && (
                <p className="aux-muted evo-inline-hint">{t("evo.autoNeedMaster")}</p>
              )}

              <div className="evo-search-cfg evo-cfg-grid">
                <label>
                  {t("evo.autoCooldown")}
                  <input
                    type="number"
                    className="aux-number-input"
                    min={60}
                    max={86400}
                    step={60}
                    value={cooldownDraft}
                    onChange={(e) => setCooldownDraft(e.target.value)}
                    onBlur={commitAutoNums}
                    disabled={!settings}
                    aria-label={t("evo.autoCooldown")}
                  />
                </label>
                <label>
                  {t("evo.autoMinDecisions")}
                  <input
                    type="number"
                    className="aux-number-input"
                    min={1}
                    max={50}
                    value={minDecDraft}
                    onChange={(e) => setMinDecDraft(e.target.value)}
                    onBlur={commitAutoNums}
                    disabled={!settings}
                    aria-label={t("evo.autoMinDecisions")}
                  />
                </label>
                <label>
                  {t("evo.autoMaxRuns")}
                  <input
                    type="number"
                    className="aux-number-input"
                    min={1}
                    max={24}
                    value={maxRunsDraft}
                    onChange={(e) => setMaxRunsDraft(e.target.value)}
                    onBlur={commitAutoNums}
                    disabled={!settings}
                    aria-label={t("evo.autoMaxRuns")}
                  />
                </label>
              </div>
              <p className="aux-muted evo-inline-hint">{t("evo.autoCostHint")}</p>

              {autoStatusError && (
                <div className="aux-error">
                  <AlertTriangle size={16} />
                  {autoStatusError}
                </div>
              )}
              {autoStatus && (
                <div
                  className={`evo-auto-banner${autoStatus.wouldRun ? " is-ready" : ""}`}
                  role="status"
                >
                  {t("evo.autoStatusLine")
                    .replace("{runs}", String(autoStatus.state.runsToday ?? 0))
                    .replace("{max}", String(autoStatus.maxRunsPerDay))
                    .replace("{new}", String(autoStatus.newDecisions))
                    .replace("{need}", String(autoStatus.minNewDecisions))
                    .replace(
                      "{gate}",
                      autoStatus.wouldRun
                        ? t("evo.autoWouldRun")
                        : (autoStatus.skipMessage ?? t("evo.autoSkipped")),
                    )}
                </div>
              )}
            </section>

            <section className="prefs-card aux-list-card evo-card">
              <div className="aux-list-head">
                <div>
                  <h2 className="prefs-card-title evo-card-title">
                    <span className="evo-card-title-icon" aria-hidden>
                      <ClipboardList size={15} />
                    </span>
                    {t("evo.curatorCfgTitle")}
                  </h2>
                  <p className="prefs-card-sub">{t("evo.curatorCfgSub")}</p>
                </div>
              </div>

              <article className="aux-task-row evo-compact-row">
                <div className="aux-task-icon">
                  <ClipboardList size={18} />
                </div>
                <div className="aux-task-main">
                  <div className="aux-task-titleline">
                    <h3>{t("evo.curatorCfgEnabled")}</h3>
                  </div>
                  <p>{t("evo.curatorCfgEnabledDesc")}</p>
                </div>
                <div className="aux-task-actions">
                  <button
                    type="button"
                    role="switch"
                    className="tool-toggle"
                    aria-checked={settings?.curator.enabled ?? false}
                    aria-label={t("evo.curatorCfgEnabled")}
                    onClick={() =>
                      settings &&
                      void setCurator(
                        !settings.curator.enabled,
                        settings.curator.intervalDays,
                        settings.curator.maxEnqueue,
                        settings.curator.llmDiagnose,
                        settings.curator.maxLlmCalls,
                      ).then(() => reloadCurator())
                    }
                    disabled={loading || !settings}
                  >
                    <span className="tool-toggle-thumb" />
                  </button>
                </div>
              </article>

              <article className="aux-task-row evo-compact-row">
                <div className="aux-task-icon">
                  <Sparkles size={18} />
                </div>
                <div className="aux-task-main">
                  <div className="aux-task-titleline">
                    <h3>{t("evo.curatorLlmDiagnose")}</h3>
                  </div>
                  <p>{t("evo.curatorLlmDiagnoseDesc")}</p>
                </div>
                <div className="aux-task-actions">
                  <button
                    type="button"
                    role="switch"
                    className="tool-toggle"
                    aria-checked={settings?.curator.llmDiagnose ?? false}
                    aria-label={t("evo.curatorLlmDiagnose")}
                    onClick={() =>
                      settings &&
                      void setCurator(
                        settings.curator.enabled,
                        settings.curator.intervalDays,
                        settings.curator.maxEnqueue,
                        !settings.curator.llmDiagnose,
                        settings.curator.maxLlmCalls,
                      ).then(() => reloadCurator())
                    }
                    disabled={loading || !settings}
                  >
                    <span className="tool-toggle-thumb" />
                  </button>
                </div>
              </article>

              <div className="evo-search-cfg evo-cfg-grid">
                <label>
                  {t("evo.curatorInterval")}
                  <input
                    type="number"
                    className="aux-number-input"
                    min={1}
                    max={90}
                    value={curatorIntervalDraft}
                    onChange={(e) => setCuratorIntervalDraft(e.target.value)}
                    onBlur={commitCuratorNums}
                    disabled={!settings}
                    aria-label={t("evo.curatorInterval")}
                  />
                </label>
                <label>
                  {t("evo.curatorMaxEnqueue")}
                  <input
                    type="number"
                    className="aux-number-input"
                    min={1}
                    max={20}
                    value={curatorMaxEnqueueDraft}
                    onChange={(e) => setCuratorMaxEnqueueDraft(e.target.value)}
                    onBlur={commitCuratorNums}
                    disabled={!settings}
                    aria-label={t("evo.curatorMaxEnqueue")}
                  />
                </label>
                <label>
                  {t("evo.curatorMaxLlm")}
                  <input
                    type="number"
                    className="aux-number-input"
                    min={1}
                    max={20}
                    value={curatorMaxLlmDraft}
                    onChange={(e) => setCuratorMaxLlmDraft(e.target.value)}
                    onBlur={commitCuratorNums}
                    disabled={!settings}
                    aria-label={t("evo.curatorMaxLlm")}
                  />
                </label>
              </div>
              <p className="aux-muted evo-inline-hint">{t("evo.curatorCfgHint")}</p>
              {curatorStatus?.due && (
                <div className="evo-auto-banner is-ready" role="status">
                  {t("evo.curatorDueBanner")
                    .replace(
                      "{detail}",
                      curatorStatus.skipMessage ?? t("evo.curatorDueDefault"),
                    )}
                </div>
              )}
              {curatorStatus && !curatorStatus.due && curatorStatus.enabled && (
                <p className="aux-muted evo-inline-hint">
                  {curatorStatus.skipMessage ??
                    t("evo.curatorNotDue")
                      .replace("{days}", String(curatorStatus.daysSinceLast ?? 0))
                      .replace("{interval}", String(curatorStatus.intervalDays))}
                </p>
              )}
            </section>
          </div>
        </>
      )}

      {section === "run" && (
        <section className="prefs-card aux-list-card evo-card">
          <div className="aux-list-head">
            <div>
              <h2 className="prefs-card-title evo-card-title">
                <span className="evo-card-title-icon" aria-hidden>
                  <Beaker size={15} />
                </span>
                {t("evo.proposalsTitle")}
              </h2>
              <p className="prefs-card-sub">{t("evo.proposalsSub")}</p>
            </div>
            <div className="aux-list-head-actions">
              <button
                type="button"
                className="aux-action aux-action-ghost"
                onClick={() => void runEvolution().then(() => reloadHistory())}
                disabled={running || !settings?.enabled}
              >
                <Play size={15} />
                {running ? t("evo.running") : t("evo.run")}
              </button>
              <button
                type="button"
                className="aux-action"
                onClick={() =>
                  void runSearch(searchFocusSkill.trim() || null).then(() => reloadHistory())
                }
                disabled={running || !settings?.enabled}
                title={t("evo.searchRunHint")}
              >
                <Dna size={15} />
                {running && runMode === "search" ? t("evo.runningSearch") : t("evo.searchRun")}
              </button>
              {running && runMode === "search" && (
                <button
                  type="button"
                  className="aux-action aux-action-ghost"
                  onClick={() => void cancelSearch()}
                >
                  {t("evo.cancelSearch")}
                </button>
              )}
            </div>
          </div>

          {running && (
            <p className="aux-muted evo-running-banner" role="status">
              {runMode === "search" ? t("evo.runningSearchHint") : t("evo.runningReflectHint")}
            </p>
          )}

          <div className="evo-run-panel">
            <div className="evo-focus-row">
              <label className="evo-focus-label" title={t("evo.focusSkillDesc")}>
                {t("evo.focusSkill")}
                <SelectMenu
                  value={searchFocusSkill}
                  options={searchFocusOptions}
                  onChange={setSearchFocusSkill}
                  aria-label={t("evo.focusSkill")}
                />
              </label>
            </div>
            <div className="evo-search-cfg evo-cfg-grid">
              <label>
                {t("evo.generations")}
                <input
                  type="number"
                  className="aux-number-input"
                  min={1}
                  max={6}
                  value={genDraft}
                  onChange={(e) => setGenDraft(e.target.value)}
                  onBlur={commitSearch}
                  disabled={!settings}
                />
              </label>
              <label>
                {t("evo.variants")}
                <input
                  type="number"
                  className="aux-number-input"
                  min={1}
                  max={6}
                  value={varDraft}
                  onChange={(e) => setVarDraft(e.target.value)}
                  onBlur={commitSearch}
                  disabled={!settings}
                />
              </label>
              <label title={t("evo.populationSizeDesc")}>
                {t("evo.populationSize")}
                <input
                  type="number"
                  className="aux-number-input"
                  min={1}
                  max={8}
                  value={popDraft}
                  onChange={(e) => setPopDraft(e.target.value)}
                  onBlur={commitSearch}
                  disabled={!settings}
                />
              </label>
              <label title={t("evo.maxEvalExamplesDesc")}>
                {t("evo.maxEvalExamples")}
                <input
                  type="number"
                  className="aux-number-input"
                  min={0}
                  max={32}
                  value={evalExDraft}
                  onChange={(e) => setEvalExDraft(e.target.value)}
                  onBlur={commitSearch}
                  disabled={!settings}
                />
              </label>
              <label title={t("evo.maxLlmCallsDesc")}>
                {t("evo.maxLlmCalls")}
                <input
                  type="number"
                  className="aux-number-input"
                  min={0}
                  max={500}
                  value={llmDraft}
                  onChange={(e) => setLlmDraft(e.target.value)}
                  onBlur={commitSearch}
                  disabled={!settings}
                />
              </label>
              <label className="evo-cfg-toggle">
                {t("evo.crossover")}
                <button
                  type="button"
                  role="switch"
                  className="tool-toggle"
                  aria-checked={settings?.search.crossover ?? false}
                  aria-label={t("evo.crossover")}
                  onClick={toggleCrossover}
                  disabled={!settings}
                >
                  <span className="tool-toggle-thumb" />
                </button>
              </label>
            </div>
            <p className="aux-muted evo-inline-hint">{t("evo.searchCostHint")}</p>
          </div>

          {lastSearch && (
            <p className="evo-run-summary">
              {t("evo.searchSummary")
                .replace("{gen}", String(lastSearch.generations))
                .replace("{evaluated}", String(lastSearch.variantsEvaluated))
                .replace("{kept}", String(lastSearch.paretoKept))
                .replace("{proposals}", String(lastSearch.proposals.length))}
              {lastSearch.budgetUsed > 0
                ? ` · LLM ${lastSearch.budgetUsed}${lastSearch.termination ? ` · ${lastSearch.termination}` : ""}`
                : ""}
              {lastSearch.holdoutEnabled ? ` · ${t("evo.holdoutUsed")}` : ""}
              {lastSearch.sandboxUsed
                ? ` · ${t("evo.sandboxUsed").replace("{n}", String(lastSearch.sandboxSkills))}`
                : ""}
              {lastSearch.focusSkill
                ? ` · ${t("evo.focusUsed").replace("{skill}", lastSearch.focusSkill)}`
                : ""}
            </p>
          )}

          {!settings?.enabled && (
            <p className="aux-muted evo-inline-hint">{t("evo.disabledHint")}</p>
          )}
          {evoError && (
            <div className="aux-error">
              <AlertTriangle size={16} />
              {evoError}
            </div>
          )}
          {branchMsg && <p className="aux-muted evo-inline-hint">{branchMsg}</p>}
          {lastReport && (
            <p className="evo-run-summary">
              {t("evo.runSummary")
                .replace("{generated}", String(lastReport.generated))
                .replace("{gated}", String(lastReport.gatedOut))
                .replace("{judged}", String(lastReport.judgedOut))
                .replace("{proposals}", String(lastReport.proposals.length))}
            </p>
          )}

          {proposals.length === 0 ? (
            <div className="evo-empty">
              <Beaker size={22} strokeWidth={1.8} aria-hidden />
              <p>{t("evo.noProposals")}</p>
            </div>
          ) : (
            <div className="aux-task-list evo-proposal-list">
              {proposals.map((p) => {
                const expanded = expandedProposals.has(p.id);
                const newContent = p.content ?? "";
                const oldS = p.oldString ?? "";
                const newS = p.newString ?? "";
                const truncated =
                  p.kind === "new_skill" || p.kind === "disable" || p.kind === "merge"
                    ? newContent.length > 1200
                    : oldS.length > 400 || newS.length > 400;
                const diffText =
                  p.kind === "patch"
                    ? expanded || !truncated
                      ? `- ${oldS}\n+ ${newS}`
                      : `- ${oldS.slice(0, 400)}…\n+ ${newS.slice(0, 400)}…`
                    : expanded || !truncated
                      ? newContent
                      : `${newContent.slice(0, 1200)}…`;
                const kindLabel =
                  p.kind === "new_skill"
                    ? t("evo.kindNew")
                    : p.kind === "patch"
                      ? t("evo.kindPatch")
                      : p.kind === "disable"
                        ? t("evo.kindDisable")
                        : t("evo.kindMerge");
                return (
                <article className="aux-task-row evo-proposal-card" key={p.id}>
                  <div className="aux-task-icon">
                    {p.kind === "new_skill" ? (
                      <FilePlus2 size={18} />
                    ) : p.kind === "patch" ? (
                      <Pencil size={18} />
                    ) : (
                      <ClipboardList size={18} />
                    )}
                  </div>
                  <div className="aux-task-main">
                    <div className="aux-task-titleline">
                      <h3>{p.skillId}</h3>
                      <span className="aux-route-pill">{kindLabel}</span>
                      {p.judgeScore != null && (
                        <span className="evo-score-pill">
                          {t("evo.judgeScore")} {p.judgeScore.toFixed(2)}
                        </span>
                      )}
                    </div>
                    {p.rationale && <p>{p.rationale}</p>}
                    {p.judgeReason && <p className="aux-muted">{p.judgeReason}</p>}
                    {p.judgeScore != null && (
                      <div
                        className="evo-score-bar"
                        aria-hidden
                        style={
                          {
                            "--score": String(Math.max(0, Math.min(1, p.judgeScore))),
                          } as CSSProperties
                        }
                      >
                        <span />
                      </div>
                    )}
                    <pre className="evo-proposal-diff">{diffText}</pre>
                    {truncated && (
                      <button
                        type="button"
                        className="aux-action aux-action-ghost evo-diff-toggle"
                        onClick={() =>
                          setExpandedProposals((prev) => {
                            const next = new Set(prev);
                            if (next.has(p.id)) next.delete(p.id);
                            else next.add(p.id);
                            return next;
                          })
                        }
                      >
                        {expanded ? t("evo.collapseDiff") : t("evo.expandDiff")}
                      </button>
                    )}
                  </div>
                  <div className="aux-task-actions evo-proposal-actions">
                    <button
                      type="button"
                      className="aux-action aux-action-ghost"
                      onClick={() => void reject(p.id)}
                    >
                      <Trash2 size={15} />
                      {t("evo.reject")}
                    </button>
                    {(p.kind === "new_skill" || p.kind === "patch") && (
                      <button
                        type="button"
                        className="aux-action aux-action-ghost"
                        onClick={() =>
                          void approveToBranch(p.id).then((m) => m && setBranchMsg(m))
                        }
                      >
                        <GitBranch size={15} />
                        {t("evo.approveToBranch")}
                      </button>
                    )}
                    <button
                      type="button"
                      className="aux-action"
                      onClick={() => void approve(p.id)}
                    >
                      <Check size={15} />
                      {p.kind === "disable"
                        ? t("evo.approveDisable")
                        : p.kind === "merge"
                          ? t("evo.approveMerge")
                          : t("evo.approve")}
                    </button>
                  </div>
                </article>
                );
              })}
            </div>
          )}
        </section>
      )}

      {section === "history" && (
        <section className="prefs-card aux-list-card evo-card">
          <div className="aux-list-head">
            <div>
              <h2 className="prefs-card-title evo-card-title">
                <span className="evo-card-title-icon" aria-hidden>
                  <History size={15} />
                </span>
                {t("evo.historyTitle")}
              </h2>
              <p className="prefs-card-sub">{t("evo.historySub")}</p>
            </div>
          </div>

          <div className="evo-stats">
            <div className="evo-stat evo-stat-primary">
              <span className="evo-stat-num">{history?.summary.totalRuns ?? 0}</span>
              <span className="evo-stat-label">{t("evo.statRuns")}</span>
            </div>
            <div className="evo-stat">
              <span className="evo-stat-num">{history?.summary.totalProposals ?? 0}</span>
              <span className="evo-stat-label">{t("evo.statProposals")}</span>
            </div>
            <div className="evo-stat">
              <span className="evo-stat-num">
                {history ? `${Math.round(history.summary.adoptionRate * 100)}%` : "—"}
              </span>
              <span className="evo-stat-label">{t("evo.statAdoption")}</span>
            </div>
            <div className="evo-stat">
              <span className="evo-stat-num">
                {history && history.summary.avgAdoptedScore > 0
                  ? history.summary.avgAdoptedScore.toFixed(2)
                  : "—"}
              </span>
              <span className="evo-stat-label">{t("evo.statAvgScore")}</span>
            </div>
            <div className="evo-stat">
              <span className="evo-stat-num">
                {history
                  ? `${history.summary.approved + history.summary.branched}/${history.summary.rejected}`
                  : "—"}
              </span>
              <span className="evo-stat-label">{t("evo.statAdoptedRejected")}</span>
            </div>
          </div>

          {history && history.summary.scoreTrend.length >= 2 && (
            <div className="evo-trend">
              <span className="evo-stat-label">{t("evo.trend")}</span>
              <svg className="evo-spark" viewBox="0 0 160 36" preserveAspectRatio="none" aria-hidden>
                <defs>
                  <linearGradient id="evoSparkFill" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="0%" stopColor="var(--tone)" stopOpacity="0.28" />
                    <stop offset="100%" stopColor="var(--tone)" stopOpacity="0" />
                  </linearGradient>
                </defs>
                <polyline
                  className="evo-spark-area"
                  points={`0,36 ${history.summary.scoreTrend
                    .map((v, i, a) => {
                      const x = a.length > 1 ? (i / (a.length - 1)) * 160 : 0;
                      const clamped = Math.max(0, Math.min(1, v));
                      const y = 35 - clamped * 33;
                      return `${x.toFixed(1)},${y.toFixed(1)}`;
                    })
                    .join(" ")} 160,36`}
                />
                <polyline
                  points={history.summary.scoreTrend
                    .map((v, i, a) => {
                      const x = a.length > 1 ? (i / (a.length - 1)) * 160 : 0;
                      const clamped = Math.max(0, Math.min(1, v));
                      const y = 35 - clamped * 33;
                      return `${x.toFixed(1)},${y.toFixed(1)}`;
                    })
                    .join(" ")}
                />
              </svg>
            </div>
          )}

          {history && history.recent.length > 0 ? (
            <div className="aux-task-list evo-history-list">
              {history.recent.slice(0, 12).map((ev, i) => {
                const type = String(ev.type ?? "");
                const meta = ev.search_meta as Record<string, unknown> | undefined;
                const line =
                  type === "run"
                    ? meta && String(ev.mode ?? "") === "search"
                      ? t("evo.historySearchLine")
                          .replace("{mode}", String(ev.mode ?? ""))
                          .replace("{proposals}", String(ev.proposals ?? 0))
                          .replace("{budget}", String(meta.budget_used ?? "?"))
                          .replace("{limit}", String(meta.budget_limit ?? "?"))
                          .replace("{term}", String(meta.termination ?? ""))
                          .replace(
                            "{holdout}",
                            meta.holdout_enabled ? t("evo.holdoutOn") : t("evo.holdoutOff"),
                          )
                      : `run · ${String(ev.mode ?? "")} · gen ${Number(ev.generated ?? 0)} → 提案 ${Number(
                          ev.proposals ?? 0,
                        )}`
                    : `${String(ev.outcome ?? "")} · ${String(ev.skill_id ?? "")}${
                        ev.score != null ? ` · ${Number(ev.score).toFixed(2)}` : ""
                      }`;
                return (
                  <article
                    className={`aux-task-row evo-history-row evo-history-${type || "event"}`}
                    key={`${type}-${i}`}
                  >
                    <div className="evo-history-kind" aria-hidden>
                      {type === "run" ? <Play size={14} /> : <Check size={14} />}
                    </div>
                    <div className="aux-task-main">
                      <p>{line}</p>
                    </div>
                  </article>
                );
              })}
            </div>
          ) : (
            <div className="evo-empty">
              <History size={22} strokeWidth={1.8} aria-hidden />
              <p>{t("evo.historyEmpty")}</p>
            </div>
          )}
        </section>
      )}

      {section === "lab" && (
        <div className="evo-split">
          <section className="prefs-card aux-list-card evo-card">
            <div className="aux-list-head">
              <div>
                <h2 className="prefs-card-title evo-card-title">
                  <span className="evo-card-title-icon" aria-hidden>
                    <FlaskConical size={15} />
                  </span>
                  {t("evo.evalTitle")}
                </h2>
                <p className="prefs-card-sub">{t("evo.evalSub")}</p>
              </div>
            </div>

            {evalError && (
              <div className="aux-error">
                <AlertTriangle size={16} />
                {evalError}
              </div>
            )}

            <div className="evo-eval-form">
              <input
                className="evo-eval-input"
                placeholder={t("evo.evalTaskPlaceholder")}
                value={evalTask}
                onChange={(e) => setEvalTask(e.target.value)}
              />
              <SelectMenu
                value={evalSkill}
                options={evalSkillOptions}
                onChange={setEvalSkill}
                aria-label={t("evo.evalSkillPlaceholder")}
              />
              <textarea
                className="evo-eval-input evo-eval-textarea"
                placeholder={t("evo.evalExpectPlaceholder")}
                value={evalExpect}
                onChange={(e) => setEvalExpect(e.target.value)}
                rows={3}
              />
              <div className="aux-task-actions">
                <button
                  type="button"
                  className={`aux-action aux-action-ghost evo-verdict-btn${
                    evalVerdict === "pass" ? " is-pass" : " is-fail"
                  }`}
                  onClick={() => setEvalVerdict(evalVerdict === "fail" ? "pass" : "fail")}
                >
                  {evalVerdict === "fail" ? t("evo.evalVerdictFail") : t("evo.evalVerdictPass")}
                </button>
                <button
                  type="button"
                  className="aux-action"
                  onClick={submitEval}
                  disabled={!evalTask.trim()}
                >
                  <Check size={15} />
                  {t("evo.evalAdd")}
                </button>
              </div>
            </div>

            <div className="evo-eval-import">
              <div className="evo-eval-import-head">
                <p className="aux-muted evo-inline-hint">{t("evo.evalImportHint")}</p>
                <button
                  type="button"
                  className="aux-action aux-action-ghost"
                  onClick={() => void reloadImportCandidates()}
                  disabled={importLoading}
                >
                  {importLoading ? t("evo.evalImportScanning") : t("evo.evalImportScan")}
                </button>
              </div>
              {importCandidates.length > 0 && (
                <div className="aux-task-list evo-eval-import-list">
                  {importCandidates.map((cand) => (
                    <article className="aux-task-row evo-compact-row" key={cand.sessionId}>
                      <div className="aux-task-main">
                        <div className="aux-task-titleline">
                          <h3>{cand.task}</h3>
                          <span className="aux-route-pill is-auto">
                            {t("evo.evalImportFailCount").replace("{n}", String(cand.failCount))}
                          </span>
                        </div>
                        {cand.expectations.length > 0 && (
                          <p className="aux-muted">{cand.expectations.join(" · ")}</p>
                        )}
                        <p className="aux-muted evo-eval-import-session">{cand.sessionId}</p>
                      </div>
                      <div className="aux-task-actions">
                        <button
                          type="button"
                          className="aux-action"
                          onClick={() =>
                            void importFromSession(cand.sessionId, evalSkill.trim() || null)
                          }
                        >
                          <Check size={15} />
                          {t("evo.evalImportBtn")}
                        </button>
                      </div>
                    </article>
                  ))}
                </div>
              )}
            </div>

            {evalExamples.length === 0 ? (
              <p className="aux-muted evo-inline-hint">{t("evo.evalEmpty")}</p>
            ) : (
              <div className="aux-task-list">
                {evalExamples.map((ex) => (
                  <article className="aux-task-row evo-compact-row" key={ex.id}>
                    <div className="aux-task-main">
                      <div className="aux-task-titleline">
                        <h3>{ex.task}</h3>
                        <span
                          className={`aux-route-pill${ex.verdict === "pass" ? " is-pass" : " is-auto"}`}
                        >
                          {ex.verdict === "pass"
                            ? t("evo.evalVerdictPass")
                            : t("evo.evalVerdictFail")}
                        </span>
                        {ex.skillId && <span className="aux-route-pill">{ex.skillId}</span>}
                      </div>
                      {ex.expectations.length > 0 && (
                        <p className="aux-muted">{ex.expectations.join(" · ")}</p>
                      )}
                    </div>
                    <div className="aux-task-actions">
                      <button
                        type="button"
                        className="aux-action aux-action-ghost"
                        onClick={() => void removeEval(ex.id)}
                      >
                        <Trash2 size={15} />
                        {t("evo.reject")}
                      </button>
                    </div>
                  </article>
                ))}
              </div>
            )}
          </section>

          <section className="prefs-card aux-list-card evo-card">
            <div className="aux-list-head">
              <div>
                <h2 className="prefs-card-title evo-card-title">
                  <span className="evo-card-title-icon" aria-hidden>
                    <ClipboardList size={15} />
                  </span>
                  {t("evo.curatorTitle")}
                </h2>
                <p className="prefs-card-sub">{t("evo.curatorSub")}</p>
              </div>
              <div className="aux-list-head-actions">
                <label className="evo-cfg-toggle" title={t("evo.curatorEnqueueDesc")}>
                  {t("evo.curatorEnqueue")}
                  <button
                    type="button"
                    role="switch"
                    className="tool-toggle"
                    aria-checked={curatorEnqueue}
                    onClick={() => setCuratorEnqueue((v) => !v)}
                  >
                    <span className="tool-toggle-thumb" />
                  </button>
                </label>
                <button
                  type="button"
                  className="aux-action"
                  onClick={() =>
                    void runCurator(curatorEnqueue).then(() => {
                      if (curatorEnqueue) void reloadProposals();
                    })
                  }
                  disabled={curatorLoading}
                >
                  <RefreshCw size={15} />
                  {curatorLoading ? t("evo.curatorRunning") : t("evo.curatorRun")}
                </button>
              </div>
            </div>

            {curatorError && (
              <div className="aux-error">
                <AlertTriangle size={16} />
                {curatorError}
              </div>
            )}

            {curatorAutoNotice != null && (
              <div className="evo-auto-banner is-ready evo-curator-notice" role="status">
                <span>
                  {t("evo.curatorAutoRefreshed").replace("{n}", curatorAutoNotice)}
                </span>
                <button
                  type="button"
                  className="aux-action aux-action-ghost"
                  onClick={() => clearCuratorAutoNotice()}
                >
                  {t("evo.dismiss")}
                </button>
              </div>
            )}

            {curatorStatus?.due && (
              <div className="evo-auto-banner is-ready" role="status">
                {t("evo.curatorDueBanner").replace(
                  "{detail}",
                  curatorStatus.skipMessage ?? t("evo.curatorDueDefault"),
                )}
              </div>
            )}

            {!curatorReport ? (
              <p className="aux-muted evo-inline-hint">{t("evo.curatorEmpty")}</p>
            ) : (
              <>
                <p className="evo-run-summary">
                  {t("evo.curatorSummary")
                    .replace("{n}", String(curatorReport.enabledCount))
                    .replace("{stale}", String(curatorReport.stale.length))
                    .replace("{sug}", String(curatorReport.suggestions.length))}
                  {lastEnqueued > 0
                    ? ` · ${t("evo.curatorEnqueued").replace("{n}", String(lastEnqueued))}`
                    : ""}
                </p>
                {(curatorReport.overlapClusters?.length ?? 0) > 0 && (
                  <div className="evo-curator-block">
                    <h3 className="evo-curator-block-title">{t("evo.curatorOverlapTitle")}</h3>
                    <ul className="evo-curator-overlap">
                      {curatorReport.overlapClusters!.map((cluster, i) => (
                        <li key={`overlap-${i}`}>
                          {t("evo.curatorOverlapLine")
                            .replace("{i}", String(i + 1))
                            .replace("{ids}", cluster.join(" · "))}
                        </li>
                      ))}
                    </ul>
                  </div>
                )}
                {(() => {
                  const rows = [...(curatorReport.rows ?? [])]
                    .filter((r) => r.stale || r.healthScore != null || r.healthReasons.length > 0)
                    .sort((a, b) => {
                      const sa = a.healthScore ?? 2;
                      const sb = b.healthScore ?? 2;
                      return sa - sb;
                    })
                    .slice(0, 12);
                  if (rows.length === 0) return null;
                  return (
                    <div className="evo-curator-block">
                      <h3 className="evo-curator-block-title">{t("evo.curatorHealthTitle")}</h3>
                      <div className="aux-task-list">
                        {rows.map((row) => (
                          <article
                            className="aux-task-row evo-compact-row"
                            key={`health-${row.skillId}`}
                          >
                            <div className="aux-task-main">
                              <div className="aux-task-titleline">
                                <h3>{row.skillId}</h3>
                                {row.stale && (
                                  <span className="aux-route-pill is-auto">
                                    {t("evo.curatorStaleTag")}
                                  </span>
                                )}
                                <span className="evo-score-pill">
                                  {row.healthScore != null
                                    ? t("evo.curatorHealthScore").replace(
                                        "{score}",
                                        row.healthScore.toFixed(2),
                                      )
                                    : t("evo.curatorHealthUnknown")}
                                </span>
                                {row.bytes != null && (
                                  <span className="aux-muted">
                                    {t("evo.curatorBytes").replace("{n}", String(row.bytes))}
                                  </span>
                                )}
                              </div>
                              {row.description && (
                                <p className="aux-muted">{row.description}</p>
                              )}
                              {row.healthReasons.length > 0 && (
                                <p className="aux-muted">{row.healthReasons.join(" · ")}</p>
                              )}
                            </div>
                          </article>
                        ))}
                      </div>
                    </div>
                  );
                })()}
                {curatorReport.suggestions.some(
                  (s) => s.kind === "disable" || s.kind === "merge",
                ) && (
                  <div className="aux-task-actions" style={{ marginBottom: 8 }}>
                    <button
                      type="button"
                      className="aux-action aux-action-ghost"
                      onClick={() => void enqueueCurator().then(() => reloadProposals())}
                      disabled={curatorLoading}
                    >
                      <Inbox size={15} />
                      {t("evo.curatorEnqueueBtn")}
                    </button>
                  </div>
                )}
                {curatorReport.suggestions.length > 0 && (
                  <div className="aux-task-list">
                    {curatorReport.suggestions.map((s, i) => (
                      <article className="aux-task-row evo-compact-row" key={`${s.kind}-${s.skillId}-${i}`}>
                        <div className="aux-task-main">
                          <div className="aux-task-titleline">
                            <h3>{s.skillId}</h3>
                            <span className="aux-route-pill">
                              {s.kind === "disable"
                                ? t("evo.curatorDisable")
                                : s.kind === "merge"
                                  ? t("evo.curatorMerge")
                                  : t("evo.curatorRewrite")}
                            </span>
                          </div>
                          <p className="aux-muted">{s.reason}</p>
                        </div>
                        {s.kind === "rewrite" && (
                          <div className="aux-task-actions">
                            <button
                              type="button"
                              className="aux-action aux-action-ghost"
                              onClick={() => {
                                setSearchFocusSkill(s.skillId);
                                setSection("run");
                              }}
                            >
                              <Dna size={15} />
                              {t("evo.curatorEvolve")}
                            </button>
                          </div>
                        )}
                      </article>
                    ))}
                  </div>
                )}
              </>
            )}
          </section>

          <section className="prefs-card aux-list-card evo-card">
            <div className="aux-list-head">
              <div>
                <h2 className="prefs-card-title evo-card-title">
                  <span className="evo-card-title-icon" aria-hidden>
                    <Sparkles size={15} />
                  </span>
                  {t("evo.dspyTitle")}
                </h2>
                <p className="prefs-card-sub">{t("evo.dspySub")}</p>
              </div>
            </div>

            {dspyError && (
              <div className="aux-error">
                <AlertTriangle size={16} />
                {dspyError}
              </div>
            )}
            {dspyMessage && <p className="aux-muted evo-inline-hint">{dspyMessage}</p>}

            <div className="evo-dspy-status">
              <span className={`evo-dspy-chip${dspyStatus?.pythonOk ? " is-ok" : ""}`}>
                Python {dspyStatus?.pythonOk ? "OK" : "—"}
              </span>
              <span className={`evo-dspy-chip${dspyStatus?.dspyInstalled ? " is-ok" : ""}`}>
                DSPy {dspyStatus?.dspyInstalled ? t("evo.dspyReady") : t("evo.dspyMissing")}
              </span>
              <span className={`evo-dspy-chip${dspyStatus?.enabled ? " is-ok" : ""}`}>
                {dspyStatus?.enabled ? t("evo.on") : t("evo.off")}
              </span>
            </div>
            {dspyStatus?.projectPath && (
              <p className="aux-muted evo-path">{dspyStatus.projectPath}</p>
            )}

            <div className="evo-eval-form">
              <input
                className="evo-eval-input"
                placeholder={t("evo.dspySkillPlaceholder")}
                value={dspySkill}
                onChange={(e) => setDspySkill(e.target.value)}
              />
              <div className="aux-task-actions evo-dspy-actions">
                <button
                  type="button"
                  className="aux-action aux-action-ghost"
                  onClick={() => void setupDspy()}
                  disabled={dspyBusy || dspyStatus?.dspyInstalled}
                >
                  {t("evo.dspySetup")}
                </button>
                <button
                  type="button"
                  className="aux-action aux-action-ghost"
                  onClick={() =>
                    void runDspy(dspySkill.trim(), true).then((r) => {
                      if (r) {
                        void reloadProposals();
                        void reloadHistory();
                      }
                    })
                  }
                  disabled={dspyBusy || !dspySkill.trim()}
                  title={t("evo.dspyMockHint")}
                >
                  {t("evo.dspyMock")}
                </button>
                <button
                  type="button"
                  className="aux-action"
                  onClick={() =>
                    void runDspy(dspySkill.trim()).then((r) => {
                      if (r) {
                        void reloadProposals();
                        void reloadHistory();
                      }
                    })
                  }
                  disabled={dspyBusy || !dspySkill.trim() || !dspyStatus?.enabled}
                >
                  <Dna size={15} />
                  {dspyBusy ? t("evo.running") : t("evo.dspyRun")}
                </button>
              </div>
            </div>
            <p className="aux-muted evo-inline-hint">{t("evo.dspyNote")}</p>
          </section>
        </div>
      )}

      <p className="aux-muted evo-phase-note">{t("evo.phaseNote")}</p>
    </div>
  );
}
