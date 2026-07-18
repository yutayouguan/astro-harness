import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  AlertTriangle,
  Check,
  Dna,
  FilePlus2,
  Gavel,
  GitBranch,
  Pencil,
  Play,
  RefreshCw,
  Sparkles,
  Trash2,
  type LucideIcon,
} from "lucide-react";
import { useEvolutionSettings } from "../../hooks/settings/useEvolutionSettings";
import { useEvolutionProposals } from "../../hooks/settings/useEvolutionProposals";
import { useEvalExamples } from "../../hooks/settings/useEvalExamples";
import { useDspy } from "../../hooks/settings/useDspy";
import { useI18n } from "../../i18n/LocaleContext";
import type { MessageKey } from "../../i18n/messages";
import type {
  EvolutionRouteDto,
  EvolutionRouteId,
  ModelInfo,
  ProviderDto,
  ProviderModelsResult,
  ProvidersStateDto,
} from "../../types";
import { SelectMenu } from "../ui/SelectMenu";

type Props = {
  active: boolean;
};

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
    reload,
  } = useEvolutionSettings(active);
  const {
    running,
    error: evoError,
    lastReport,
    lastSearch,
    proposals,
    run: runEvolution,
    runSearch,
    approve,
    approveToBranch,
    reject,
    reload: reloadProposals,
  } = useEvolutionProposals(active);
  const [branchMsg, setBranchMsg] = useState<string | null>(null);
  const [genDraft, setGenDraft] = useState("");
  const [varDraft, setVarDraft] = useState("");
  const {
    examples: evalExamples,
    add: addEval,
    remove: removeEval,
    error: evalError,
  } = useEvalExamples(active);
  const [evalTask, setEvalTask] = useState("");
  const [evalSkill, setEvalSkill] = useState("");
  const [evalExpect, setEvalExpect] = useState("");
  const [evalVerdict, setEvalVerdict] = useState<"fail" | "pass">("fail");
  const {
    status: dspyStatus,
    busy: dspyBusy,
    error: dspyError,
    message: dspyMessage,
    setup: setupDspy,
    run: runDspy,
  } = useDspy(active);
  const [dspySkill, setDspySkill] = useState("");

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
    }
  }, [settings]);

  const commitSearch = useCallback(() => {
    if (!settings) return;
    const g = Number.parseInt(genDraft, 10);
    const v = Number.parseInt(varDraft, 10);
    const gen = Number.isFinite(g) && g >= 1 && g <= 6 ? g : settings.search.generations;
    const vari = Number.isFinite(v) && v >= 1 && v <= 6 ? v : settings.search.variants;
    if (gen !== settings.search.generations || vari !== settings.search.variants) {
      void setSearch(gen, vari, settings.search.crossover);
    } else {
      setGenDraft(String(settings.search.generations));
      setVarDraft(String(settings.search.variants));
    }
  }, [settings, genDraft, varDraft, setSearch]);

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

  return (
    <div className="aux-page prefs-page aux-page-embedded" data-tone="blue">
      {(error || providersError) && (
        <div className="aux-error">
          <AlertTriangle size={16} />
          {error ?? providersError}
        </div>
      )}

      <section className="prefs-card aux-list-card">
        <div className="aux-list-head">
          <div>
            <h2 className="prefs-card-title">{t("evo.title")}</h2>
            <p className="prefs-card-sub">{t("evo.subtitle")}</p>
          </div>
          <div className="aux-list-head-actions">
            <button
              type="button"
              className="aux-action aux-action-ghost"
              onClick={() => void reload()}
              disabled={loading}
            >
              <RefreshCw size={15} />
              {t("aux.refresh")}
            </button>
          </div>
        </div>

        <article className="aux-task-row">
          <div className="aux-task-icon">
            <Dna size={18} />
          </div>
          <div className="aux-task-main">
            <div className="aux-task-titleline">
              <h3>{t("evo.enabled")}</h3>
              <span className={`aux-route-pill${settings?.enabled ? "" : " is-auto"}`}>
                {settings?.enabled ? t("evo.on") : t("evo.off")}
              </span>
            </div>
            <p>{t("evo.enabledDesc")}</p>
          </div>
          <div className="aux-task-actions">
            <button
              type="button"
              className="aux-action"
              onClick={() => void setEnabled(!settings?.enabled)}
              disabled={loading || !settings}
            >
              {settings?.enabled ? t("evo.disable") : t("evo.enable")}
            </button>
          </div>
        </article>

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

      <section className="prefs-card aux-list-card">
        <div className="aux-list-head">
          <div>
            <h2 className="prefs-card-title">{t("evo.gatesTitle")}</h2>
            <p className="prefs-card-sub">{t("evo.gatesSub")}</p>
          </div>
        </div>

        <article className="aux-task-row">
          <div className="aux-task-main">
            <div className="aux-task-titleline">
              <h3>{t("evo.runTests")}</h3>
            </div>
            <p>{t("evo.runTestsDesc")}</p>
          </div>
          <div className="aux-task-actions">
            <button
              type="button"
              className="aux-action"
              onClick={() =>
                gates &&
                void setGates(!gates.runTests, gates.maxSkillBytes, gates.requirePr, gates.minJudgeScore)
              }
              disabled={!gates}
            >
              {gates?.runTests ? t("evo.on") : t("evo.off")}
            </button>
          </div>
        </article>

        <article className="aux-task-row">
          <div className="aux-task-main">
            <div className="aux-task-titleline">
              <h3>{t("evo.requirePr")}</h3>
            </div>
            <p>{t("evo.requirePrDesc")}</p>
          </div>
          <div className="aux-task-actions">
            <button
              type="button"
              className="aux-action"
              onClick={() =>
                gates &&
                void setGates(gates.runTests, gates.maxSkillBytes, !gates.requirePr, gates.minJudgeScore)
              }
              disabled={!gates}
            >
              {gates?.requirePr ? t("evo.on") : t("evo.off")}
            </button>
          </div>
        </article>

        <article className="aux-task-row">
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

        <article className="aux-task-row">
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

      <section className="prefs-card aux-list-card">
        <div className="aux-list-head">
          <div>
            <h2 className="prefs-card-title">{t("evo.proposalsTitle")}</h2>
            <p className="prefs-card-sub">{t("evo.proposalsSub")}</p>
          </div>
          <div className="aux-list-head-actions">
            <button
              type="button"
              className="aux-action aux-action-ghost"
              onClick={() => void runEvolution()}
              disabled={running || !settings?.enabled}
            >
              <Play size={15} />
              {running ? t("evo.running") : t("evo.run")}
            </button>
            <button
              type="button"
              className="aux-action"
              onClick={() => void runSearch()}
              disabled={running || !settings?.enabled}
              title={t("evo.searchRunHint")}
            >
              <Dna size={15} />
              {running ? t("evo.running") : t("evo.searchRun")}
            </button>
          </div>
        </div>

        <div className="evo-search-cfg">
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
          <button
            type="button"
            className="aux-action aux-action-ghost"
            onClick={() =>
              settings &&
              void setSearch(
                settings.search.generations,
                settings.search.variants,
                !settings.search.crossover,
              )
            }
            disabled={!settings}
          >
            {t("evo.crossover")}: {settings?.search.crossover ? t("evo.on") : t("evo.off")}
          </button>
          <span className="aux-muted">{t("evo.searchCostHint")}</span>
        </div>
        {lastSearch && (
          <p className="aux-muted">
            {t("evo.searchSummary")
              .replace("{gen}", String(lastSearch.generations))
              .replace("{evaluated}", String(lastSearch.variantsEvaluated))
              .replace("{kept}", String(lastSearch.paretoKept))
              .replace("{proposals}", String(lastSearch.proposals.length))}
          </p>
        )}

        {!settings?.enabled && (
          <p className="aux-muted">{t("evo.disabledHint")}</p>
        )}
        {evoError && (
          <div className="aux-error">
            <AlertTriangle size={16} />
            {evoError}
          </div>
        )}
        {branchMsg && <p className="aux-muted">{branchMsg}</p>}
        {lastReport && (
          <p className="aux-muted">
            {t("evo.runSummary")
              .replace("{generated}", String(lastReport.generated))
              .replace("{gated}", String(lastReport.gatedOut))
              .replace("{judged}", String(lastReport.judgedOut))
              .replace("{proposals}", String(lastReport.proposals.length))}
          </p>
        )}

        {proposals.length === 0 ? (
          <p className="aux-muted">{t("evo.noProposals")}</p>
        ) : (
          <div className="aux-task-list">
            {proposals.map((p) => (
              <article className="aux-task-row" key={p.id}>
                <div className="aux-task-icon">
                  {p.kind === "new_skill" ? <FilePlus2 size={18} /> : <Pencil size={18} />}
                </div>
                <div className="aux-task-main">
                  <div className="aux-task-titleline">
                    <h3>{p.skillId}</h3>
                    <span className="aux-route-pill">
                      {p.kind === "new_skill" ? t("evo.kindNew") : t("evo.kindPatch")}
                    </span>
                    {p.judgeScore != null && (
                      <span className="aux-route-pill">
                        {t("evo.judgeScore")}: {p.judgeScore.toFixed(2)}
                      </span>
                    )}
                  </div>
                  {p.rationale && <p>{p.rationale}</p>}
                  {p.judgeReason && <p className="aux-muted">{p.judgeReason}</p>}
                  <pre className="evo-proposal-diff">
                    {p.kind === "new_skill"
                      ? (p.content ?? "").slice(0, 1200)
                      : `- ${(p.oldString ?? "").slice(0, 400)}\n+ ${(p.newString ?? "").slice(0, 400)}`}
                  </pre>
                </div>
                <div className="aux-task-actions">
                  <button
                    type="button"
                    className="aux-action aux-action-ghost"
                    onClick={() => void reject(p.id)}
                  >
                    <Trash2 size={15} />
                    {t("evo.reject")}
                  </button>
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
                  <button
                    type="button"
                    className="aux-action"
                    onClick={() => void approve(p.id)}
                  >
                    <Check size={15} />
                    {t("evo.approve")}
                  </button>
                </div>
              </article>
            ))}
          </div>
        )}
      </section>

      <section className="prefs-card aux-list-card">
        <div className="aux-list-head">
          <div>
            <h2 className="prefs-card-title">{t("evo.evalTitle")}</h2>
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
          <input
            className="evo-eval-input"
            placeholder={t("evo.evalSkillPlaceholder")}
            value={evalSkill}
            onChange={(e) => setEvalSkill(e.target.value)}
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
              className="aux-action aux-action-ghost"
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

        {evalExamples.length === 0 ? (
          <p className="aux-muted">{t("evo.evalEmpty")}</p>
        ) : (
          <div className="aux-task-list">
            {evalExamples.map((ex) => (
              <article className="aux-task-row" key={ex.id}>
                <div className="aux-task-main">
                  <div className="aux-task-titleline">
                    <h3>{ex.task}</h3>
                    <span className={`aux-route-pill${ex.verdict === "pass" ? "" : " is-auto"}`}>
                      {ex.verdict === "pass" ? t("evo.evalVerdictPass") : t("evo.evalVerdictFail")}
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

      <section className="prefs-card aux-list-card">
        <div className="aux-list-head">
          <div>
            <h2 className="prefs-card-title">{t("evo.dspyTitle")}</h2>
            <p className="prefs-card-sub">{t("evo.dspySub")}</p>
          </div>
        </div>

        {dspyError && (
          <div className="aux-error">
            <AlertTriangle size={16} />
            {dspyError}
          </div>
        )}
        {dspyMessage && <p className="aux-muted">{dspyMessage}</p>}

        <p className="aux-muted">
          {t("evo.dspyStatus")}: python {dspyStatus?.pythonOk ? "OK" : "—"} · dspy{" "}
          {dspyStatus?.dspyInstalled ? "OK" : "—"} ·{" "}
          {dspyStatus?.enabled ? t("evo.on") : t("evo.off")}
          {dspyStatus?.projectPath ? ` · ${dspyStatus.projectPath}` : ""}
        </p>

        <div className="evo-eval-form">
          <input
            className="evo-eval-input"
            placeholder={t("evo.dspySkillPlaceholder")}
            value={dspySkill}
            onChange={(e) => setDspySkill(e.target.value)}
          />
          <div className="aux-task-actions">
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
              className="aux-action"
              onClick={() =>
                void runDspy(dspySkill.trim()).then((r) => {
                  if (r) void reloadProposals();
                })
              }
              disabled={dspyBusy || !dspySkill.trim() || !dspyStatus?.enabled}
            >
              <Dna size={15} />
              {dspyBusy ? t("evo.running") : t("evo.dspyRun")}
            </button>
          </div>
        </div>
        <p className="aux-muted">{t("evo.dspyNote")}</p>
      </section>

      <p className="aux-muted evo-phase-note">{t("evo.phaseNote")}</p>
    </div>
  );
}
