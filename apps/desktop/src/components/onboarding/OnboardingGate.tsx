import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { AnimatePresence, motion, useReducedMotion } from "framer-motion";
import {
  ArrowLeft,
  Bot,
  Check,
  ChevronRight,
  FolderOpen,
  KeyRound,
  Languages,
  LoaderCircle,
  Monitor,
  Moon,
  ShieldCheck,
  Sparkles,
  Sun,
  Wifi,
} from "lucide-react";
import { useTheme, type ThemeMode } from "../../hooks/app/useTheme";
import { useI18n } from "../../i18n/LocaleContext";
import type {
  ProjectDto,
  ProviderDto,
  ProviderTestResult,
  ProviderModelsResult,
  ProvidersStateDto,
} from "../../types";
import {
  ONBOARDING_RESET_EVENT,
  EMPTY_ONBOARDING_DRAFT,
  classifyConnectionIssue,
  withDeadline,
  createOnboardingWriteQueue,
  persistableOnboardingEndpoint,
  type ConnectionIssueKind,
  type OnboardingDraft,
  inferProjectName,
  resumeOnboardingStep,
  providerConfigInput,
  providerIsReady,
  providerRequiresApiKey,
  storeOnboardingStarterPrompt,
  type OnboardingStateDto,
  type OnboardingStep,
} from "../../lib/ui/onboarding";
import { ProviderBrandIcon } from "../icons/ProviderIcons";
import { Button, SelectMenu } from "../ui";
import { OnboardingLogo, OnboardingBrandMotion } from "./OnboardingBrand";
import { StarterTaskChooser } from "./StarterTaskChooser";
import { ConnectionIssue } from "./ConnectionIssue";
import { COPY } from "./onboardingCopy";
import { onboardingModelOptions } from "../../lib/ui/onboardingModels";

const isTauri = () =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

const STEP_ORDER: OnboardingStep[] = ["personalize", "provider", "workspace"];

type PermissionPreset = "ask_for_approval" | "approve_for_me";
type ProviderStatus = "idle" | "testing" | "success" | "error";
type AppConfigSlice = {
  active_agent_id: string;
  agents: Array<{ id: string; name: string }>;
};

type FirstRunOnboardingProps = {
  initialStep?: OnboardingStep;
  onComplete: () => void;
  previewProviders?: ProviderDto[];
  disableIntroAdvance?: boolean;
  initialDraft?: OnboardingDraft;
  handoff?: boolean;
  onHandoffComplete?: () => void;
};

type HealthSummary = {
  agent: string;
  provider: string;
  workspace: string;
  permission: string;
  modelStatus: "verified" | "unavailable" | "demo";
  sandboxAvailable: boolean;
};

function stepIndex(step: OnboardingStep): number {
  return STEP_ORDER.indexOf(step);
}

function slideVariants(direction: number, reducedMotion: boolean) {
  if (reducedMotion) {
    return {
      enter: { opacity: 0 },
      center: { opacity: 1 },
      exit: { opacity: 0 },
    };
  }
  return {
    enter: { opacity: 0, x: direction >= 0 ? 52 : -52 },
    center: { opacity: 1, x: 0 },
    exit: { opacity: 0, x: direction >= 0 ? -52 : 52 },
  };
}

export function FirstRunOnboarding({
  initialStep = "intro",
  onComplete,
  previewProviders,
  disableIntroAdvance = false,
  initialDraft = EMPTY_ONBOARDING_DRAFT,
  handoff = false,
  onHandoffComplete,
}: FirstRunOnboardingProps) {
  const { locale, setLocale } = useI18n();
  const { mode, setMode } = useTheme();
  const reducedMotion = useReducedMotion() ?? false;
  const copy = COPY[locale];
  const [step, setStep] = useState<OnboardingStep>(initialStep);
  const [direction, setDirection] = useState(1);
  const [agentName, setAgentName] = useState(initialDraft.agent_name);
  const [currentAgentName, setCurrentAgentName] = useState("Astro");
  const [providersState, setProvidersState] =
    useState<ProvidersStateDto | null>(
      previewProviders
        ? {
            providers: previewProviders,
            active_provider_id: previewProviders[0]?.id ?? null,
            active_image_provider_id: null,
          }
        : null,
    );
  const [selectedProviderId, setSelectedProviderId] = useState(
    initialDraft.provider_id,
  );
  const [apiKey, setApiKey] = useState("");
  const [model, setModel] = useState(initialDraft.model);
  const [endpoint, setEndpoint] = useState(initialDraft.endpoint);
  const [modelOptions, setModelOptions] = useState<
    Array<{ value: string; label: string }>
  >([]);
  const [modelsStatus, setModelsStatus] = useState<
    "idle" | "loading" | "ready" | "error"
  >("idle");
  const [providerStatus, setProviderStatus] = useState<ProviderStatus>("idle");
  const [providerMessage, setProviderMessage] = useState("");
  const [workspacePath, setWorkspacePath] = useState(
    initialDraft.workspace_path,
  );
  const [permissionPreset, setPermissionPreset] = useState<PermissionPreset>(
    initialDraft.permission_preset,
  );
  const [finishing, setFinishing] = useState(false);
  const [finishError, setFinishError] = useState("");
  const [demoDraftPrepared, setDemoDraftPrepared] = useState(false);
  const [connectionIssue, setConnectionIssue] =
    useState<ConnectionIssueKind | null>(null);
  const [progressError, setProgressError] = useState(false);
  const providerRequest = useRef(0);
  const mounted = useRef(true);
  const writes = useRef(createOnboardingWriteQueue());
  const apiKeyRef = useRef<HTMLInputElement>(null);
  const modelInputRef = useRef<HTMLDivElement>(null);
  const endpointInputRef = useRef<HTMLInputElement>(null);
  const providerSelectRef = useRef<HTMLDivElement>(null);
  const identityLoadedRef = useRef(false);
  const verified = useRef<{
    id: string;
    model: string;
    endpoint: string;
    token: string;
  } | null>(null);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      providerRequest.current += 1;
    };
  }, []);
  const [healthSummary, setHealthSummary] = useState<HealthSummary | null>(
    () =>
      initialStep === "complete"
        ? {
            agent: "Astro",
            provider: `${previewProviders?.[0]?.display_name ?? copy.provider} · ${previewProviders?.[0]?.model ?? "—"}`,
            workspace: copy.defaultWorkspace,
            permission: copy.ask,
            modelStatus: "demo",
            sandboxAvailable: true,
          }
        : null,
  );
  const focusHeading = useCallback((node: HTMLHeadingElement | null) => {
    node?.focus({ preventScroll: true });
  }, []);
  const agentNameTouchedRef = useRef(initialDraft.agent_name !== "Astro");

  const providers = useMemo(
    () =>
      [
        ...(providersState?.providers ?? []),
        ...(providersState?.provider_templates ?? []),
      ].filter((provider) => provider.supports_responses_api === true),
    [providersState],
  );
  const selectedProvider =
    providers.find((provider) => provider.id === selectedProviderId) ?? null;

  const draft: OnboardingDraft = {
    agent_name: agentName,
    provider_id: selectedProviderId,
    model,
    endpoint,
    workspace_path: workspacePath,
    permission_preset: permissionPreset,
  };
  const draftRef = useRef(draft);
  draftRef.current = draft;
  const saveProgress = useCallback(
    (next: OnboardingStep) => {
      if (previewProviders || !isTauri() || next === "complete")
        return Promise.resolve();
      const snapshot = {
        ...draftRef.current,
        endpoint: persistableOnboardingEndpoint(draftRef.current.endpoint),
      };
      return writes.current
        .run(() =>
          withDeadline(
            invoke<OnboardingStateDto>("save_onboarding_progress", {
              step: next,
              draft: snapshot,
            }),
          ),
        )
        .then(
          () => {
            if (mounted.current) setProgressError(false);
          },
          () => {
            if (mounted.current) setProgressError(true);
            throw new Error("progress");
          },
        );
    },
    [previewProviders],
  );
  useEffect(() => {
    if (step === "intro" || step === "complete" || finishing || handoff) return;
    const timer = window.setTimeout(() => {
      void saveProgress(step).catch(() => {});
    }, 180);
    return () => window.clearTimeout(timer);
  }, [
    step,
    agentName,
    selectedProviderId,
    model,
    endpoint,
    workspacePath,
    permissionPreset,
    saveProgress,
    finishing,
    handoff,
  ]);
  const goTo = useCallback(
    (next: OnboardingStep) => {
      if (step === "provider" && next === "personalize") {
        providerRequest.current += 1;
        verified.current = null;
        setProviderStatus("idle");
      }
      setDirection(stepIndex(next) >= stepIndex(step) ? 1 : -1);
      setStep(next);
    },
    [step],
  );

  useEffect(() => {
    if (step !== "intro" || handoff) return;
    const timer = disableIntroAdvance
      ? undefined
      : window.setTimeout(
          () => goTo("personalize"),
          reducedMotion ? 200 : 2200,
        );
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") goTo("personalize");
    };
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.clearTimeout(timer);
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [disableIntroAdvance, goTo, reducedMotion, step, handoff]);

  useEffect(() => {
    if (previewProviders || !isTauri()) return;
    let disposed = false;
    void withDeadline(invoke<ProvidersStateDto>("get_providers_state"))
      .then((state) => {
        if (!disposed) setProvidersState(state);
      })
      .catch((error) => {
        if (!disposed) {
          setConnectionIssue(classifyConnectionIssue(error));
          setProviderMessage("");
        }
      });
    return () => {
      disposed = true;
    };
  }, [previewProviders]);

  useEffect(() => {
    if (previewProviders || !isTauri()) return;
    let disposed = false;
    void withDeadline(invoke<AppConfigSlice>("get_config"))
      .then((config) => {
        if (disposed) return;
        const active = config.agents.find(
          (agent) => agent.id === config.active_agent_id,
        );
        if (!active?.name) return;
        identityLoadedRef.current = true;
        setCurrentAgentName(active.name);
        if (!agentNameTouchedRef.current) setAgentName(active.name);
      })
      .catch(() => {
        if (!disposed) setProgressError(true);
      });
    return () => {
      disposed = true;
    };
  }, [previewProviders]);

  useEffect(() => {
    if (providers.length === 0) return;
    const preferred =
      providers.find(
        (provider) => provider.id === providersState?.active_provider_id,
      ) ??
      providers.find(providerIsReady) ??
      providers[0];
    if (!providers.some((provider) => provider.id === selectedProviderId)) {
      setSelectedProviderId(preferred.id);
      setModel("");
      setEndpoint(preferred.endpoint);
    }
  }, [providers, providersState?.active_provider_id, selectedProviderId]);

  const selectProvider = (id: string) => {
    providerRequest.current += 1;
    verified.current = null;
    setConnectionIssue(null);
    const provider = providers.find((item) => item.id === id);
    if (!provider) return;
    setSelectedProviderId(provider.id);
    setModel("");
    setModelOptions([]);
    setModelsStatus("idle");
    setEndpoint(provider.endpoint);
    setApiKey("");
    setProviderStatus("idle");
    setProviderMessage("");
  };

  const invalidateModelList = () => {
    providerRequest.current += 1;
    verified.current = null;
    setProviderStatus("idle");
    setProviderMessage("");
    setModel("");
    setModelOptions([]);
    setModelsStatus("idle");
    setConnectionIssue(null);
  };
  const loadModels = async () => {
    if (
      !selectedProvider ||
      modelsStatus === "loading" ||
      providerStatus === "testing"
    )
      return;
    setConnectionIssue(null);
    if (!persistableOnboardingEndpoint(endpoint)) {
      setConnectionIssue("network");
      return;
    }
    if (
      providerRequiresApiKey(selectedProvider) &&
      !selectedProvider.has_api_key &&
      !apiKey.trim()
    ) {
      setConnectionIssue("credentials");
      return;
    }
    const request = ++providerRequest.current;
    const currentRequest = () =>
      mounted.current && request === providerRequest.current;
    verified.current = null;
    setProviderStatus("idle");
    setProviderMessage("");
    setModelOptions([]);
    setModelsStatus("loading");
    try {
      let options: Array<{ value: string; label: string }>;
      if (previewProviders) {
        await new Promise((resolve) => window.setTimeout(resolve, 250));
        if (!currentRequest()) return;
        options = onboardingModelOptions([{ id: selectedProvider.model }]);
        setProvidersState((state) =>
          state
            ? {
                ...state,
                providers: state.providers.map((provider) =>
                  provider.id === selectedProvider.id
                    ? { ...provider, has_api_key: true }
                    : provider,
                ),
              }
            : state,
        );
        setApiKey("");
      } else {
        const next = await withDeadline(
          invoke<ProvidersStateDto>("save_provider", {
            provider: {
              ...providerConfigInput(selectedProvider, selectedProvider.model),
              endpoint: endpoint.trim(),
            },
          }),
        );
        if (!currentRequest()) return;
        setProvidersState(next);
        if (apiKey.trim()) {
          const keyed = await withDeadline(
            invoke<ProvidersStateDto>("set_provider_api_key", {
              id: selectedProvider.id,
              apiKey: apiKey.trim(),
            }),
          );
          if (!currentRequest()) return;
          setProvidersState(keyed);
          setApiKey("");
        }
        const listed = await withDeadline(
          invoke<ProviderModelsResult>("list_provider_models", {
            id: selectedProvider.id,
            refresh: true,
          }),
        );
        if (!currentRequest()) return;
        options = onboardingModelOptions(listed.models ?? []);
      }
      if (!options.length) {
        setModelsStatus("error");
        setConnectionIssue("model");
        setModel("");
        return;
      }
      setModelOptions(options);
      setModel((current) =>
        options.some((option) => option.value === current) ? current : "",
      );
      setModelsStatus("ready");
    } catch (error) {
      if (!currentRequest()) return;
      setModelsStatus("error");
      setConnectionIssue(classifyConnectionIssue(error));
    }
  };

  const cancelProviderTest = () => {
    providerRequest.current += 1;
    setProviderStatus("idle");
    if (modelsStatus === "loading") setModelsStatus("idle");
    setProviderMessage("");
  };
  const testProvider = async () => {
    if (
      !selectedProvider ||
      providerStatus === "testing" ||
      modelsStatus === "loading"
    )
      return;
    if (
      modelsStatus !== "ready" ||
      !modelOptions.some((option) => option.value === model)
    ) {
      setConnectionIssue("model");
      return;
    }
    verified.current = null;
    setConnectionIssue(null);
    if (!persistableOnboardingEndpoint(endpoint)) {
      setConnectionIssue("network");
      return;
    }
    if (!model.trim()) {
      setConnectionIssue("model");
      return;
    }
    if (
      providerRequiresApiKey(selectedProvider) &&
      !selectedProvider.has_api_key &&
      !apiKey.trim()
    ) {
      setConnectionIssue("credentials");
      return;
    }
    const request = ++providerRequest.current;
    const currentRequest = () =>
      mounted.current && providerRequest.current === request;
    const id = selectedProvider.id,
      testedModel = model.trim(),
      testedEndpoint = endpoint.trim();
    setProviderStatus("testing");
    setProviderMessage("");
    let verificationToken: string | null = previewProviders ? "preview" : null;
    try {
      if (previewProviders) {
        await new Promise((resolve) => window.setTimeout(resolve, 300));
        if (!currentRequest()) return;
      } else {
        const next = await withDeadline(
          invoke<ProvidersStateDto>("save_provider", {
            provider: {
              ...providerConfigInput(selectedProvider, testedModel),
              endpoint: testedEndpoint,
            },
          }),
        );
        if (!currentRequest()) return;
        setProvidersState(next);
        if (apiKey.trim()) {
          const keyed = await withDeadline(
            invoke<ProvidersStateDto>("set_provider_api_key", {
              id,
              apiKey: apiKey.trim(),
            }),
          );
          if (!currentRequest()) return;
          setProvidersState(keyed);
          setApiKey("");
        }
        const result = await withDeadline(
          invoke<ProviderTestResult & { verification_token: string | null }>(
            "verify_onboarding_provider",
            {
              id,
              model: testedModel,
            },
          ),
        );
        if (!currentRequest()) return;
        if (!result.ok) throw new Error(result.message);
        if (!result.verification_token)
          throw new Error("ONBOARDING_VERIFICATION_REQUIRED");
        verificationToken = result.verification_token;
        const active = await withDeadline(
          invoke<ProvidersStateDto>("set_active_provider_model", {
            id,
            model: testedModel,
          }),
        );
        if (!currentRequest()) return;
        setProvidersState(active);
      }
      if (!verificationToken)
        throw new Error("ONBOARDING_VERIFICATION_REQUIRED");
      verified.current = {
        id,
        model: testedModel,
        endpoint: testedEndpoint,
        token: verificationToken,
      };
      setProviderStatus("success");
      setProviderMessage(
        previewProviders ? copy.demoConnected : copy.testSuccess,
      );
    } catch (error) {
      if (!currentRequest()) return;
      setProviderStatus("error");
      setConnectionIssue(classifyConnectionIssue(error));
    }
  };

  const chooseWorkspace = async () => {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        directory: true,
        multiple: false,
        title: copy.chooseFolder,
      });
      if (typeof selected === "string") setWorkspacePath(selected);
    } catch {
      // 用户取消选择时保留当前选项。
    }
  };

  const finish = async () => {
    if (finishing) return;
    if (
      !previewProviders &&
      (!verified.current || providerStatus !== "success")
    ) {
      setConnectionIssue("verification");
      goTo("provider");
      return;
    }
    setFinishing(true);
    setFinishError("");
    try {
      let projectLabel = workspacePath
        ? inferProjectName(workspacePath)
        : copy.defaultWorkspace;
      let providerLabel = `${selectedProvider?.display_name ?? copy.provider} · ${model}`;
      if (!previewProviders && isTauri()) {
        await saveProgress("workspace");
        if (
          agentName.trim() &&
          (identityLoadedRef.current || agentNameTouchedRef.current)
        ) {
          await invoke("set_default_agent_name", { name: agentName.trim() });
        }
        const permission = await invoke<{
          preset: string;
          sandboxHealth?: { status: string };
        }>("set_permission_preset", {
          preset: permissionPreset,
          confirmed: false,
        });
        const projects = await invoke<ProjectDto[]>("list_projects");
        if (workspacePath) {
          const existing = projects.find((project) =>
            project.roots.includes(workspacePath),
          );
          if (!existing) {
            const created = await invoke<ProjectDto>("create_project", {
              name: inferProjectName(workspacePath),
              roots: [workspacePath],
            });
            localStorage.setItem("astro.activeProjectId", created.id);
            projectLabel = created.name;
          } else {
            localStorage.setItem("astro.activeProjectId", existing.id);
            projectLabel = existing.name;
          }
        }
        const currentProviders = await withDeadline(
          invoke<ProvidersStateDto>("get_providers_state"),
        ).catch(() => null);
        const activeProvider = currentProviders?.providers.find(
          (provider) => provider.id === currentProviders?.active_provider_id,
        );
        if (activeProvider) {
          providerLabel = `${activeProvider.display_name} · ${activeProvider.model}`;
        }
        setHealthSummary({
          agent: agentName.trim() || currentAgentName,
          provider: providerLabel,
          workspace: projectLabel,
          permission:
            permission.preset === "approve_for_me" ? copy.approve : copy.ask,
          modelStatus: providerIsReady(activeProvider)
            ? "verified"
            : "unavailable",
          sandboxAvailable: permission.sandboxHealth?.status === "available",
        });
        await writes.current.run(() =>
          withDeadline(
            invoke<OnboardingStateDto>("complete_onboarding", {
              verificationToken: verified.current?.token,
            }),
          ),
        );
      } else {
        setHealthSummary({
          agent: agentName.trim() || currentAgentName,
          provider: providerLabel,
          workspace: projectLabel,
          permission:
            permissionPreset === "approve_for_me" ? copy.approve : copy.ask,
          modelStatus: "demo",
          sandboxAvailable: true,
        });
      }
      setStep("complete");
    } catch (error) {
      setFinishError(copy.finishFailed);
      setFinishing(false);
      if (String(error).includes("ONBOARDING_VERIFICATION_REQUIRED")) {
        verified.current = null;
        setProviderStatus("idle");
        setConnectionIssue("verification");
        goTo("provider");
      }
    }
  };

  const variants = slideVariants(direction, reducedMotion);
  const currentIndex = Math.max(0, stepIndex(step));
  const enterAstro = (prompt?: string) => {
    if (previewProviders) {
      setDemoDraftPrepared(true);
      return;
    }
    if (prompt) storeOnboardingStarterPrompt(prompt);
    onComplete();
  };

  return (
    <main
      className={`onboarding-root ${handoff ? "onboarding-root--handoff" : ""}`}
      data-step={step}
    >
      <OnboardingBrandMotion
        phase={handoff ? "app" : step === "intro" ? "intro" : "header"}
        onArrive={onHandoffComplete}
      />
      {step !== "intro" && (
        <div className="onboarding-brand-rail" aria-hidden>
          <div className="onboarding-brand">
            <div
              className="onboarding-header-anchor"
              data-onboarding-brand-anchor="header"
            />
            <span>Astro Agent</span>
          </div>
        </div>
      )}
      {previewProviders && (
        <span className="onboarding-demo-label">{copy.demoLabel}</span>
      )}
      <div className="onboarding-ambient" aria-hidden>
        <span />
        <span />
        <span />
      </div>

      {step === "intro" ? (
        <button
          type="button"
          className="onboarding-skip"
          onClick={() => goTo("personalize")}
        >
          {copy.skipIntro}
        </button>
      ) : null}

      <AnimatePresence mode="wait" initial={false} custom={direction}>
        {step === "intro" ? (
          <motion.section
            key="intro"
            className="onboarding-intro"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0, scale: reducedMotion ? 1 : 0.96 }}
            transition={{ duration: reducedMotion ? 0.2 : 0.42 }}
          >
            <div
              className="onboarding-intro-anchor"
              data-onboarding-brand-anchor="intro"
              aria-hidden
            />
            <motion.div
              className="onboarding-intro-copy"
              initial={reducedMotion ? { opacity: 0 } : { opacity: 0, y: 16 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ delay: reducedMotion ? 0 : 0.82, duration: 0.5 }}
            >
              <h1>{copy.hello}</h1>
              <p>{copy.introSub}</p>
            </motion.div>
          </motion.section>
        ) : (
          <motion.section
            key={step}
            className={`onboarding-stage onboarding-stage--${step}`}
            custom={direction}
            variants={variants}
            initial="enter"
            animate="center"
            exit="exit"
            transition={
              reducedMotion
                ? { duration: 0.18 }
                : { type: "spring", bounce: 0, duration: 0.4 }
            }
          >
            <header className="onboarding-header" data-tauri-drag-region="true">
              <div className="onboarding-brand-placeholder" aria-hidden />
              {step !== "complete" ? (
                <ol className="onboarding-progress" aria-label="Setup progress">
                  {STEP_ORDER.map((item, index) => (
                    <li
                      key={item}
                      className={index <= currentIndex ? "is-active" : ""}
                      aria-current={item === step ? "step" : undefined}
                    >
                      <span>{index + 1}</span>
                    </li>
                  ))}
                </ol>
              ) : null}
            </header>

            <div className="onboarding-card">
              {progressError && (
                <p
                  className="onboarding-status"
                  data-status="error"
                  role="alert"
                >
                  {copy.progressFailed}
                  <button
                    type="button"
                    onClick={() => void saveProgress(step).catch(() => {})}
                  >
                    {copy.retry}
                  </button>
                </p>
              )}
              {step === "personalize" ? (
                <>
                  <div className="onboarding-heading">
                    <span className="onboarding-eyebrow">
                      <Sparkles size={14} aria-hidden />
                      {copy.personalizeEyebrow}
                    </span>
                    <h1 ref={focusHeading} tabIndex={-1}>
                      {copy.personalizeTitle}
                    </h1>
                    <p>{copy.personalizeSub}</p>
                  </div>

                  <div className="onboarding-setting-group">
                    <div className="onboarding-setting-label">
                      <Languages size={18} aria-hidden />
                      <span>{copy.language}</span>
                    </div>
                    <div className="onboarding-choice-grid is-two">
                      {(["zh", "en"] as const).map((value) => (
                        <button
                          key={value}
                          type="button"
                          className={locale === value ? "is-selected" : ""}
                          aria-pressed={locale === value}
                          onClick={() => setLocale(value)}
                        >
                          <span>
                            {value === "zh" ? copy.chinese : copy.english}
                          </span>
                          {locale === value ? (
                            <Check size={16} aria-hidden />
                          ) : null}
                        </button>
                      ))}
                    </div>
                  </div>

                  <div className="onboarding-setting-group">
                    <div className="onboarding-setting-label">
                      {mode === "dark" ? (
                        <Moon size={18} aria-hidden />
                      ) : mode === "light" ? (
                        <Sun size={18} aria-hidden />
                      ) : (
                        <Monitor size={18} aria-hidden />
                      )}
                      <span>{copy.theme}</span>
                    </div>
                    <div className="onboarding-choice-grid is-three">
                      {(
                        [
                          ["light", copy.light, Sun],
                          ["auto", copy.auto, Monitor],
                          ["dark", copy.dark, Moon],
                        ] as const
                      ).map(([value, label, Icon]) => (
                        <button
                          key={value}
                          type="button"
                          className={mode === value ? "is-selected" : ""}
                          aria-pressed={mode === value}
                          onClick={() => setMode(value as ThemeMode)}
                        >
                          <Icon size={17} aria-hidden />
                          <span>{label}</span>
                        </button>
                      ))}
                    </div>
                  </div>

                  <label className="onboarding-agent-name">
                    <span className="onboarding-setting-label">
                      <Bot size={18} aria-hidden />
                      <span>{copy.agentName}</span>
                    </span>
                    <input
                      value={agentName}
                      maxLength={64}
                      placeholder={copy.agentNamePlaceholder}
                      autoComplete="off"
                      onChange={(event) => {
                        agentNameTouchedRef.current = true;
                        setAgentName(event.target.value);
                      }}
                    />
                  </label>
                </>
              ) : null}

              {step === "provider" ? (
                <>
                  <div className="onboarding-heading">
                    <span className="onboarding-eyebrow">
                      <KeyRound size={14} aria-hidden />
                      {copy.providerEyebrow}
                    </span>
                    <h1 ref={focusHeading} tabIndex={-1}>
                      {copy.providerTitle}
                    </h1>
                    <p>{copy.providerSub}</p>
                  </div>

                  {providers.length > 0 ? (
                    <div className="onboarding-provider-form">
                      <div
                        className="onboarding-provider-field"
                        ref={providerSelectRef}
                      >
                        <span>{copy.provider}</span>
                        <SelectMenu
                          className="onboarding-provider-select"
                          aria-label={copy.provider}
                          value={selectedProviderId}
                          disabled={
                            providerStatus === "testing" ||
                            modelsStatus === "loading"
                          }
                          onChange={selectProvider}
                          options={providers.map((provider) => ({
                            value: provider.id,
                            label: provider.display_name,
                            icon: <ProviderBrandIcon kind={provider.kind} />,
                          }))}
                        />
                      </div>

                      <label>
                        <span>{copy.endpoint}</span>
                        <input
                          ref={endpointInputRef}
                          value={endpoint}
                          disabled={
                            providerStatus === "testing" ||
                            modelsStatus === "loading"
                          }
                          spellCheck={false}
                          onChange={(event) => {
                            invalidateModelList();
                            setEndpoint(event.target.value);
                          }}
                        />
                      </label>

                      {selectedProvider &&
                      providerRequiresApiKey(selectedProvider) ? (
                        <label>
                          <span>{copy.apiKey}</span>
                          <input
                            ref={apiKeyRef}
                            type="password"
                            disabled={
                              providerStatus === "testing" ||
                              modelsStatus === "loading"
                            }
                            value={apiKey}
                            placeholder={
                              selectedProvider.has_api_key
                                ? copy.apiKeyStored
                                : copy.apiKeyPlaceholder
                            }
                            autoComplete="off"
                            spellCheck={false}
                            onChange={(event) => {
                              invalidateModelList();
                              setApiKey(event.target.value);
                            }}
                          />
                        </label>
                      ) : null}

                      <Button
                        className="onboarding-load-models"
                        variant="secondary"
                        busy={modelsStatus === "loading"}
                        busyLabel={copy.loadingModels}
                        disabled={
                          providerStatus === "testing" ||
                          (!apiKey.trim() &&
                            selectedProvider != null &&
                            providerRequiresApiKey(selectedProvider) &&
                            !selectedProvider.has_api_key)
                        }
                        onClick={() => void loadModels()}
                      >
                        {modelsStatus === "loading"
                          ? copy.loadingModels
                          : copy.loadModels}
                      </Button>
                      <p className="onboarding-model-hint">
                        {copy.modelLoadHint}
                      </p>
                      <div
                        className="onboarding-provider-field"
                        ref={modelInputRef}
                      >
                        <span>{copy.model}</span>
                        <SelectMenu
                          className="onboarding-provider-select"
                          aria-label={copy.model}
                          value={model}
                          options={modelOptions}
                          placeholder={copy.chooseModel}
                          disabled={
                            modelsStatus !== "ready" ||
                            providerStatus === "testing"
                          }
                          onChange={(value) => {
                            setModel(value);
                            setProviderStatus("idle");
                            verified.current = null;
                            setConnectionIssue(null);
                          }}
                        />
                      </div>

                      <p className="onboarding-test-cost">{copy.testCost}</p>
                      <button
                        type="button"
                        className="onboarding-test-button"
                        data-status={providerStatus}
                        disabled={
                          providerStatus === "testing" ||
                          modelsStatus !== "ready" ||
                          !model
                        }
                        onClick={() => void testProvider()}
                      >
                        {providerStatus === "testing" ? (
                          <LoaderCircle
                            className="is-spinning"
                            size={17}
                            aria-hidden
                          />
                        ) : providerStatus === "success" ? (
                          <Check size={17} aria-hidden />
                        ) : (
                          <Wifi size={17} aria-hidden />
                        )}
                        <span>
                          {providerStatus === "testing"
                            ? copy.testing
                            : copy.test}
                        </span>
                      </button>
                      {(providerStatus === "testing" ||
                        modelsStatus === "loading") && (
                        <button
                          type="button"
                          className="onboarding-cancel-test"
                          onClick={cancelProviderTest}
                        >
                          {copy.cancelTest}
                        </button>
                      )}
                      {connectionIssue && (
                        <ConnectionIssue
                          kind={connectionIssue}
                          locale={locale}
                          onRetry={() =>
                            void (modelsStatus === "ready" && model
                              ? testProvider()
                              : loadModels())
                          }
                          onEdit={() => {
                            if (
                              connectionIssue === "model" &&
                              modelsStatus === "ready"
                            )
                              modelInputRef.current
                                ?.querySelector<HTMLButtonElement>(
                                  ".select-menu-trigger",
                                )
                                ?.focus();
                            else if (connectionIssue === "credentials")
                              (
                                apiKeyRef.current ??
                                providerSelectRef.current?.querySelector<HTMLButtonElement>(
                                  ".select-menu-trigger",
                                )
                              )?.focus();
                            else if (
                              ["quota", "rate_limit"].includes(connectionIssue)
                            )
                              providerSelectRef.current
                                ?.querySelector<HTMLButtonElement>(
                                  ".select-menu-trigger",
                                )
                                ?.focus();
                            else {
                              endpointInputRef.current?.focus();
                            }
                          }}
                        />
                      )}
                      {providerMessage ? (
                        <p
                          className="onboarding-status"
                          data-status={providerStatus}
                          role={providerStatus === "error" ? "alert" : "status"}
                        >
                          {providerMessage}
                        </p>
                      ) : null}
                    </div>
                  ) : (
                    <div className="onboarding-empty" role="status">
                      {providersState
                        ? copy.noProviders
                        : copy.loadingProviders}
                      {connectionIssue && (
                        <ConnectionIssue
                          kind={connectionIssue}
                          locale={locale}
                          onRetry={() => {
                            void withDeadline(
                              invoke<ProvidersStateDto>("get_providers_state"),
                            )
                              .then((state) => {
                                setProvidersState(state);
                                setConnectionIssue(null);
                              })
                              .catch((error) =>
                                setConnectionIssue(
                                  classifyConnectionIssue(error),
                                ),
                              );
                          }}
                        />
                      )}
                    </div>
                  )}
                </>
              ) : null}

              {step === "workspace" ? (
                <>
                  <div className="onboarding-heading">
                    <span className="onboarding-eyebrow">
                      <FolderOpen size={14} aria-hidden />
                      {copy.workspaceEyebrow}
                    </span>
                    <h1 ref={focusHeading} tabIndex={-1}>
                      {copy.workspaceTitle}
                    </h1>
                    <p>{copy.workspaceSub}</p>
                  </div>

                  <button
                    type="button"
                    className="onboarding-folder-picker"
                    onClick={() => void chooseWorkspace()}
                  >
                    <span className="onboarding-folder-icon">
                      <FolderOpen size={22} aria-hidden />
                    </span>
                    <span>
                      <strong>
                        {workspacePath
                          ? inferProjectName(workspacePath)
                          : copy.defaultWorkspace}
                      </strong>
                      <small>{workspacePath || copy.chooseFolder}</small>
                    </span>
                    <span className="onboarding-folder-action">
                      {workspacePath ? copy.changeFolder : copy.chooseFolder}
                    </span>
                  </button>

                  <fieldset className="onboarding-permissions">
                    <legend>
                      <ShieldCheck size={18} aria-hidden />
                      {copy.permission}
                    </legend>
                    <label
                      className={
                        permissionPreset === "ask_for_approval"
                          ? "is-selected"
                          : ""
                      }
                    >
                      <input
                        type="radio"
                        name="onboarding-permission"
                        checked={permissionPreset === "ask_for_approval"}
                        onChange={() => setPermissionPreset("ask_for_approval")}
                      />
                      <span>
                        <strong>{copy.ask}</strong>
                        <small>{copy.askSub}</small>
                      </span>
                      <Check size={17} aria-hidden />
                    </label>
                    <label
                      className={
                        permissionPreset === "approve_for_me"
                          ? "is-selected"
                          : ""
                      }
                    >
                      <input
                        type="radio"
                        name="onboarding-permission"
                        checked={permissionPreset === "approve_for_me"}
                        onChange={() => setPermissionPreset("approve_for_me")}
                      />
                      <span>
                        <strong>{copy.approve}</strong>
                        <small>{copy.approveSub}</small>
                      </span>
                      <Check size={17} aria-hidden />
                    </label>
                  </fieldset>
                  <p className="onboarding-network-note">
                    <ShieldCheck size={15} aria-hidden />
                    {copy.networkNote}
                  </p>
                  {finishError ? (
                    <p
                      className="onboarding-status"
                      data-status="error"
                      role="alert"
                    >
                      {finishError}
                    </p>
                  ) : null}
                </>
              ) : null}

              {step === "complete" ? (
                <div className="onboarding-complete">
                  <div className="onboarding-complete-mark">
                    <Check size={34} strokeWidth={2.2} aria-hidden />
                  </div>
                  <h1 ref={focusHeading} tabIndex={-1}>
                    {healthSummary?.modelStatus === "verified"
                      ? copy.completeTitle
                      : copy.savedTitle}
                  </h1>
                  <p>
                    {healthSummary?.modelStatus === "verified"
                      ? copy.completeSub
                      : copy.savedSub}
                  </p>

                  {healthSummary && (
                    <div className="onboarding-health-grid" role="status">
                      <div>
                        <Check size={15} aria-hidden />
                        <span>
                          <small>{copy.healthAgent}</small>
                          <strong>{healthSummary.agent}</strong>
                        </span>
                      </div>
                      <div
                        data-status={
                          healthSummary.modelStatus === "verified"
                            ? "ok"
                            : "pending"
                        }
                      >
                        <span aria-hidden>
                          {healthSummary.modelStatus === "verified" ? "✓" : "○"}
                        </span>
                        <span>
                          <small>{copy.healthModel}</small>
                          <strong>
                            {copy.modelStatuses[healthSummary.modelStatus]}
                          </strong>
                          {!["unavailable"].includes(
                            healthSummary.modelStatus,
                          ) && <small>{healthSummary.provider}</small>}
                        </span>
                      </div>
                      <div>
                        <Check size={15} aria-hidden />
                        <span>
                          <small>{copy.healthWorkspace}</small>
                          <strong>{healthSummary.workspace}</strong>
                        </span>
                      </div>
                      <div
                        data-status={
                          healthSummary.sandboxAvailable ? "ok" : "pending"
                        }
                      >
                        <span aria-hidden>
                          {healthSummary.sandboxAvailable ? "✓" : "○"}
                        </span>
                        <span>
                          <small>{copy.healthPermission}</small>
                          <strong>{healthSummary.permission}</strong>
                          {!healthSummary.sandboxAvailable && (
                            <small>{copy.sandboxUnavailable}</small>
                          )}
                        </span>
                      </div>
                    </div>
                  )}
                  <StarterTaskChooser locale={locale} onUse={enterAstro} />
                  {demoDraftPrepared && (
                    <p role="status">
                      {locale === "zh"
                        ? "演示草稿已准备，未保存或发送。"
                        : "Demo draft prepared; not saved or sent."}
                    </p>
                  )}
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => enterAstro()}
                  >
                    {copy.enter}
                    <ChevronRight size={15} aria-hidden />
                  </Button>
                </div>
              ) : null}

              {step !== "complete" ? (
                <footer className="onboarding-actions">
                  {step !== "personalize" ? (
                    <Button
                      variant="ghost"
                      disabled={finishing}
                      onClick={() =>
                        goTo(step === "workspace" ? "provider" : "personalize")
                      }
                    >
                      <ArrowLeft size={16} aria-hidden />
                      {copy.back}
                    </Button>
                  ) : (
                    <span />
                  )}
                  {step === "workspace" ? (
                    <Button
                      variant="primary"
                      busy={finishing}
                      disabled={
                        !previewProviders &&
                        (providerStatus !== "success" || !verified.current)
                      }
                      busyLabel={copy.finishing}
                      onClick={() => void finish()}
                    >
                      {copy.finish}
                      <ChevronRight size={16} aria-hidden />
                    </Button>
                  ) : (
                    <Button
                      variant="primary"
                      disabled={
                        step === "provider" && providerStatus !== "success"
                      }
                      onClick={() =>
                        goTo(step === "personalize" ? "provider" : "workspace")
                      }
                    >
                      {copy.continue}
                      <ChevronRight size={16} aria-hidden />
                    </Button>
                  )}
                </footer>
              ) : null}
            </div>
          </motion.section>
        )}
      </AnimatePresence>
    </main>
  );
}

export default function OnboardingGate({ children }: { children: ReactNode }) {
  const { locale } = useI18n();
  const [state, setState] = useState<
    "loading" | "visible" | "entering" | "ready" | "error"
  >(isTauri() ? "loading" : "ready");
  const [initialState, setInitialState] = useState<OnboardingStateDto | null>(
    null,
  );
  const [epoch, setEpoch] = useState(0);
  useEffect(() => {
    if (!isTauri()) return;
    let disposed = false;
    setState("loading");
    void withDeadline(invoke<OnboardingStateDto>("get_onboarding_state"))
      .then((next) => {
        if (!disposed) {
          setInitialState(next);
          setState(next.should_show ? "visible" : "ready");
        }
      })
      .catch(() => {
        if (!disposed) setState("error");
      });
    return () => {
      disposed = true;
    };
  }, [epoch]);
  useEffect(() => {
    const reset = () => setEpoch((value) => value + 1);
    window.addEventListener(ONBOARDING_RESET_EVENT, reset);
    return () => window.removeEventListener(ONBOARDING_RESET_EVENT, reset);
  }, []);
  const arrive = useCallback(() => setState("ready"), []);
  if (state === "loading")
    return (
      <main
        className="onboarding-root onboarding-root--loading"
        aria-busy="true"
      >
        <OnboardingLogo compact />
      </main>
    );
  if (state === "error")
    return (
      <main className="onboarding-root onboarding-root--loading">
        <section className="onboarding-card">
          <h1>
            {locale === "zh"
              ? "暂时无法读取初始化状态"
              : "Setup state is unavailable"}
          </h1>
          <p>
            {locale === "zh"
              ? "请重试读取状态。已有配置不会被覆盖；初始化需要先验证模型连接。"
              : "Retry loading setup. Existing configuration is preserved; a verified model is required."}
          </p>
          <Button onClick={() => setEpoch((value) => value + 1)}>
            {locale === "zh" ? "重试" : "Retry"}
          </Button>
        </section>
      </main>
    );
  return (
    <>
      {(state === "visible" || state === "entering") && (
        <FirstRunOnboarding
          key={epoch}
          initialStep={resumeOnboardingStep(initialState?.step ?? "intro")}
          initialDraft={initialState?.draft}
          handoff={state === "entering"}
          onHandoffComplete={arrive}
          onComplete={() => setState("entering")}
        />
      )}
      {(state === "entering" || state === "ready") && (
        <div className="onboarding-app-host" key="app">
          {children}
        </div>
      )}
    </>
  );
}
