import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { InterfaceTourReady } from "./InterfaceTourContext";
import { AnimatePresence, motion, useReducedMotion } from "framer-motion";
import {
  ArrowLeft,
  Bot,
  Check,
  ChevronRight,
  CircleAlert,
  CircleCheck,
  CircleX,
  Clock3,
  Cpu,
  FolderOpen,
  Info,
  KeyRound,
  Languages,
  Link2,
  ListRestart,
  LoaderCircle,
  MessageSquare,
  Monitor,
  Moon,
  RefreshCw,
  RotateCcw,
  Server,
  ShieldAlert,
  ShieldCheck,
  ShieldQuestion,
  Sparkles,
  Sun,
  Zap,
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
import {
  OnboardingLogo,
  OnboardingBrandMotion,
  OnboardingIntroParticles,
} from "./OnboardingBrand";
import { StarterTaskChooser } from "./StarterTaskChooser";
import { OnboardingWarp } from "./OnboardingWarp";
import { OnboardingScene } from "./OnboardingScene";
import {
  onboardingStageVariants,
  ONBOARDING_WARP_MS,
} from "../../lib/ui/onboardingMotion";
import { OnboardingPreferences } from "./OnboardingPreferences";
import { DesktopPreferencePreviewContext } from "../../hooks/settings/useDesktopPreference";
import { ConnectionIssue } from "./ConnectionIssue";
import { COPY } from "./onboardingCopy";
import { onboardingModelOptions } from "../../lib/ui/onboardingModels";

const isTauri = () =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

const STEP_ORDER: OnboardingStep[] = ["personalize", "provider", "workspace"];

type PermissionPreset = "ask_for_approval" | "approve_for_me";
type WorkspaceStatus =
  | "ready"
  | "missing"
  | "not_directory"
  | "not_writable"
  | "unavailable";
type WorkspaceCheck = { path: string; status: WorkspaceStatus };
type ProviderStatus = "idle" | "testing" | "success" | "error";
type AppConfigSlice = {
  default_workspace_dir: string;
  default_workspace_display_path: string;
  active_agent_id: string;
  agents: Array<{ id: string; name: string }>;
};

type FirstRunOnboardingProps = {
  initialStep?: OnboardingStep;
  onComplete: () => void;
  previewProviders?: ProviderDto[];
  onPreviewEnter?: (prompt: string) => void;
  disableIntroAdvance?: boolean;
  initialDraft?: OnboardingDraft;
  handoff?: boolean;
  onHandoffComplete?: () => void;
};

type HealthSummary = {
  agent: string;
  provider: string;
  workspace: string;
  workspacePath: string;
  permission: string;
  modelStatus: "verified" | "unavailable" | "demo";
  sandboxAvailable: boolean;
};

function stepIndex(step: OnboardingStep): number {
  return STEP_ORDER.indexOf(step);
}

export function FirstRunOnboarding({
  initialStep = "intro",
  onComplete,
  previewProviders,
  onPreviewEnter,
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
  const [petChoice, setPetChoice] = useState<boolean | null>(
    initialDraft.pet_enabled ?? null,
  );
  const [previewUsage, setPreviewUsage] = useState({
    notifications: false,
    autostart: false,
  });
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
  const [modelOpenRequest, setModelOpenRequest] = useState(0);
  const modelOpenSequence = useRef(0);
  const [focusTest, setFocusTest] = useState(false);
  const testButtonRef = useRef<HTMLButtonElement>(null);
  const continueButtonRef = useRef<HTMLButtonElement>(null);
  const [providerStatus, setProviderStatus] = useState<ProviderStatus>("idle");
  const [providerMessage, setProviderMessage] = useState("");
  const [workspacePath, setWorkspacePath] = useState(
    initialDraft.workspace_path,
  );
  const [defaultWorkspace, setDefaultWorkspace] = useState<{
    path: string;
    displayPath: string;
  } | null>(
    previewProviders
      ? { path: "~/.astro/workspace", displayPath: "~/.astro/workspace" }
      : null,
  );
  const [workspaceLoadError, setWorkspaceLoadError] = useState(false);
  const [configRetry, setConfigRetry] = useState(0);
  const [workspaceCheck, setWorkspaceCheck] = useState<WorkspaceCheck | null>(
    null,
  );
  const [workspaceCheckRetry, setWorkspaceCheckRetry] = useState(0);
  const effectiveWorkspacePath = workspacePath || defaultWorkspace?.path || "";
  const displayedWorkspacePath =
    workspacePath || defaultWorkspace?.displayPath || "";
  const workspaceReady =
    workspaceCheck?.path === effectiveWorkspacePath &&
    workspaceCheck.status === "ready";
  const WorkspaceCheckIcon = previewProviders
    ? Info
    : workspaceCheck?.path !== effectiveWorkspacePath
      ? Clock3
      : workspaceReady
        ? CircleCheck
        : CircleAlert;
  const [permissionPreset, setPermissionPreset] = useState<PermissionPreset>(
    initialDraft.permission_preset,
  );
  const [finishing, setFinishing] = useState(false);
  const [finishError, setFinishError] = useState("");
  const [demoDraft, setDemoDraft] = useState<string | null>(null);
  const demoInputRef = useRef<HTMLTextAreaElement>(null);
  const [introReplay, setIntroReplay] = useState(0);
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
            workspacePath: previewProviders ? "~/.astro/workspace" : "",
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
  const providerOptions = useMemo(
    () =>
      providers.map((provider) => ({
        value: provider.id,
        label: provider.display_name,
        icon: <ProviderBrandIcon kind={provider.kind} />,
      })),
    [providers],
  );

  const draft: OnboardingDraft = {
    agent_name: agentName,
    provider_id: selectedProviderId,
    model,
    endpoint,
    workspace_path: workspacePath,
    permission_preset: permissionPreset,
    pet_enabled: petChoice,
  };
  const draftRef = useRef(draft);
  draftRef.current = draft;
  // Form edits are local until the explicit save/load action commits them.
  // Other steps may checkpoint progress, but must not persist an unsaved URL.
  const savedProviderDraft = useRef({
    provider_id: initialDraft.provider_id,
    endpoint: initialDraft.endpoint,
    model: initialDraft.model,
  });
  const saveProgress = useCallback(
    (next: OnboardingStep) => {
      if (previewProviders || !isTauri() || next === "complete")
        return Promise.resolve();
      const snapshot = {
        ...draftRef.current,
        ...savedProviderDraft.current,
        endpoint: persistableOnboardingEndpoint(
          savedProviderDraft.current.endpoint,
        ),
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
    // Navigation checkpoints only the last explicitly saved provider values.
    if (step === "intro" || step === "complete" || handoff) return;
    void saveProgress(step).catch(() => {});
  }, [step, saveProgress, handoff]);
  useEffect(() => {
    if (
      step === "intro" ||
      step === "provider" ||
      step === "complete" ||
      finishing ||
      handoff
    )
      return;
    const timer = window.setTimeout(() => {
      void saveProgress(step).catch(() => {});
    }, 700);
    return () => window.clearTimeout(timer);
  }, [
    step,
    agentName,
    workspacePath,
    permissionPreset,
    petChoice,
    saveProgress,
    finishing,
    handoff,
  ]);
  const goTo = useCallback(
    (next: OnboardingStep) => {
      // Do not replay a consumed menu-opening request when this step remounts.
      if (step === "provider" && next !== "provider") setModelOpenRequest(0);
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
          reducedMotion ? 200 : ONBOARDING_WARP_MS.intro,
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
        if (disposed) return;
        setProvidersState(state);
        const saved = state.providers.find(
          (provider) => provider.id === initialDraft.provider_id,
        );
        if (saved) {
          // A previous draft may predate the successful model save.
          if (!initialDraft.model) setModel(saved.model);
          if (!initialDraft.endpoint) setEndpoint(saved.endpoint);
        }
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
    setWorkspaceLoadError(false);
    void withDeadline(invoke<AppConfigSlice>("get_config"))
      .then((config) => {
        if (disposed) return;
        if (
          !config.default_workspace_dir?.trim() ||
          !config.default_workspace_display_path?.trim()
        ) {
          throw new Error("Default workspace path unavailable");
        }
        setDefaultWorkspace({
          path: config.default_workspace_dir,
          displayPath: config.default_workspace_display_path,
        });
        const active = config.agents.find(
          (agent) => agent.id === config.active_agent_id,
        );
        if (!active?.name) return;
        identityLoadedRef.current = true;
        setCurrentAgentName(active.name);
        if (!agentNameTouchedRef.current) setAgentName(active.name);
      })
      .catch(() => {
        if (!disposed) setWorkspaceLoadError(true);
      });
    return () => {
      disposed = true;
    };
  }, [previewProviders, configRetry]);

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
      setModel(
        providersState?.providers.some((p) => p.id === preferred.id)
          ? preferred.model
          : "",
      );
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
    setModel(
      providersState?.providers.some((p) => p.id === provider.id)
        ? provider.model
        : "",
    );
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
  const saveProviderIfChanged = async (
    provider: ProviderDto,
    nextModel: string,
  ) => {
    if (
      provider.enabled &&
      provider.model === nextModel &&
      provider.endpoint === endpoint.trim() &&
      providersState?.providers.some((p) => p.id === provider.id)
    )
      return null;
    return withDeadline(
      invoke<ProvidersStateDto>("save_provider", {
        provider: {
          ...providerConfigInput(provider, nextModel),
          endpoint: endpoint.trim(),
        },
      }),
    );
  };
  useEffect(() => {
    if (focusTest && modelsStatus === "ready") {
      testButtonRef.current?.focus();
      setFocusTest(false);
    }
  }, [focusTest, modelsStatus]);
  useEffect(() => {
    if (providerStatus === "success") continueButtonRef.current?.focus();
  }, [providerStatus]);

  useEffect(() => {
    if (step !== "workspace" || !effectiveWorkspacePath) return;
    let disposed = false;
    setWorkspaceCheck(null);
    if (previewProviders) {
      setWorkspaceCheck({ path: effectiveWorkspacePath, status: "ready" });
    } else {
      void withDeadline(
        invoke<WorkspaceCheck>("check_onboarding_workspace", {
          path: effectiveWorkspacePath,
        }),
      )
        .then((result) => {
          if (!disposed) setWorkspaceCheck(result);
        })
        .catch(() => {
          if (!disposed)
            setWorkspaceCheck({
              path: effectiveWorkspacePath,
              status: "unavailable",
            });
        });
    }
    return () => {
      disposed = true;
    };
  }, [step, effectiveWorkspacePath, workspaceCheckRetry, previewProviders]);

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
        const next = await saveProviderIfChanged(
          selectedProvider,
          selectedProvider.model,
        );
        if (!currentRequest()) return;
        if (next) setProvidersState(next);
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
        savedProviderDraft.current = {
          provider_id: selectedProvider.id,
          endpoint: endpoint.trim(),
          model,
        };
        await saveProgress("provider");
        if (!currentRequest()) return;
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
      setModelOpenRequest(++modelOpenSequence.current);
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
        const next = await saveProviderIfChanged(selectedProvider, testedModel);
        if (!currentRequest()) return;
        if (next) setProvidersState(next);
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
      savedProviderDraft.current = {
        provider_id: id,
        endpoint: testedEndpoint,
        model: testedModel,
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
    if (!effectiveWorkspacePath || !workspaceReady) return;
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
        const checked = await withDeadline(
          invoke<WorkspaceCheck>("check_onboarding_workspace", {
            path: effectiveWorkspacePath,
          }),
        );
        setWorkspaceCheck(checked);
        if (
          checked.path !== effectiveWorkspacePath ||
          checked.status !== "ready"
        ) {
          setFinishing(false);
          return;
        }
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
        const existing = projects.find((project) =>
          project.roots.includes(effectiveWorkspacePath),
        );
        if (!existing) {
          if (!workspacePath)
            throw new Error("Default workspace project is unavailable");
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
          workspacePath: displayedWorkspacePath,
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
          workspacePath: displayedWorkspacePath,
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
      if (String(error).includes("ONBOARDING_WORKSPACE_UNAVAILABLE")) {
        setWorkspaceCheck({
          path: effectiveWorkspacePath,
          status: "unavailable",
        });
      }
      if (String(error).includes("ONBOARDING_VERIFICATION_REQUIRED")) {
        verified.current = null;
        setProviderStatus("idle");
        setConnectionIssue("verification");
        goTo("provider");
      }
    }
  };

  const variants = onboardingStageVariants(reducedMotion);
  const currentIndex = Math.max(0, stepIndex(step));
  const enterAstro = (prompt?: string) => {
    if (previewProviders) {
      if (onPreviewEnter) {
        onPreviewEnter(prompt ?? "");
        return;
      }
      setDemoDraft(prompt ?? "");
      requestAnimationFrame(() => {
        demoInputRef.current?.focus();
        demoInputRef.current?.scrollIntoView({ block: "nearest" });
      });
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
      {!handoff && (
        <OnboardingBrandMotion
          key={introReplay}
          phase={step === "intro" ? "intro" : "header"}
        />
      )}
      {step !== "intro" && (
        <div className="onboarding-brand-rail" aria-hidden>
          <div className="onboarding-brand">
            <div
              className="onboarding-header-anchor"
              data-onboarding-brand-anchor="header"
            />
            <span
              className="onboarding-header-wordmark-anchor"
              data-onboarding-wordmark-anchor="header"
            >
              Astro Agent
            </span>
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

      <OnboardingWarp
        key={`${handoff ? "app" : step}-${introReplay}`}
        mode={handoff ? "app" : step === "intro" ? "intro" : "step"}
        direction={direction}
        reduced={reducedMotion}
        onFinished={handoff ? onHandoffComplete : undefined}
      />
      {step === "intro" && <OnboardingIntroParticles key={introReplay} />}
      {step === "intro" && previewProviders && (
        <button
          type="button"
          className="onboarding-skip onboarding-replay"
          onClick={() => setIntroReplay((value) => value + 1)}
        >
          <RotateCcw size={14} aria-hidden />
          {locale === "zh" ? "重播动画" : "Replay intro"}
        </button>
      )}
      {step === "intro" ? (
        <button
          type="button"
          className="onboarding-skip"
          onClick={() => goTo("personalize")}
        >
          {copy.skipIntro}
        </button>
      ) : null}

      <div className="onboarding-scenes">
        <AnimatePresence mode="sync" initial={false} custom={direction}>
          {step === "intro" ? (
            <OnboardingScene
              key="intro"
              portal={false}
              reduced={reducedMotion}
              className="onboarding-intro"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              variants={variants}
              custom={direction}
              exit="exit"
            >
              <div
                className="onboarding-intro-anchor"
                data-onboarding-brand-anchor="intro"
                aria-hidden
              />
              <span
                className="onboarding-intro-wordmark-anchor"
                data-onboarding-wordmark-anchor="intro"
                aria-hidden
              >
                Astro Agent
              </span>
              <motion.div
                className="onboarding-intro-copy"
                initial={reducedMotion ? { opacity: 0 } : { opacity: 0, y: 16 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ delay: reducedMotion ? 0 : 0.82, duration: 0.5 }}
              >
                <h1>{copy.hello}</h1>
                <p>{copy.introSub}</p>
              </motion.div>
            </OnboardingScene>
          ) : (
            <OnboardingScene
              key={step}
              reduced={reducedMotion}
              className={`onboarding-stage onboarding-stage--${step}`}
              custom={direction}
              variants={variants}
              initial="enter"
              animate={handoff ? "exit" : "center"}
              departing={handoff}
              exit="exit"
            >
              <header
                className="onboarding-header"
                data-tauri-drag-region="true"
              >
                <div className="onboarding-brand-placeholder" aria-hidden />
                {step !== "complete" ? (
                  <ol
                    className="onboarding-progress"
                    aria-label="Setup progress"
                  >
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

                    <div className="onboarding-split onboarding-personalize-layout">
                      <div className="onboarding-column">
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
                                className={
                                  locale === value ? "is-selected" : ""
                                }
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
                      </div>
                      <DesktopPreferencePreviewContext.Provider
                        value={{
                          values: previewUsage,
                          change: (kind, value) =>
                            setPreviewUsage((current) => ({
                              ...current,
                              [kind]: value,
                            })),
                        }}
                      >
                        <OnboardingPreferences
                          locale={locale}
                          preview={Boolean(previewProviders)}
                          petChoice={petChoice}
                          onPetChoice={(value) => {
                            setPetChoice(value);
                            draftRef.current.pet_enabled = value;
                            void saveProgress(step).catch(() => {});
                          }}
                        />
                      </DesktopPreferencePreviewContext.Provider>
                    </div>
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
                      <div className="onboarding-split onboarding-provider-form">
                        <div className="onboarding-provider-column">
                          <div
                            className="onboarding-provider-field"
                            ref={providerSelectRef}
                          >
                            <span className="onboarding-field-label">
                              <Server size={14} aria-hidden />
                              {copy.provider}
                            </span>
                            <SelectMenu
                              className="onboarding-provider-select"
                              aria-label={copy.provider}
                              value={selectedProviderId}
                              disabled={
                                providerStatus === "testing" ||
                                modelsStatus === "loading"
                              }
                              onChange={selectProvider}
                              options={providerOptions}
                            />
                          </div>

                          <label>
                            <span className="onboarding-field-label">
                              <Link2 size={14} aria-hidden />
                              {copy.endpoint}
                            </span>
                            <input
                              ref={endpointInputRef}
                              data-immediate-focus
                              value={endpoint}
                              inputMode="url"
                              autoCapitalize="none"
                              autoCorrect="off"
                              autoComplete="off"
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
                              <span className="onboarding-field-label">
                                <KeyRound size={14} aria-hidden />
                                {copy.apiKey}
                              </span>
                              <input
                                ref={apiKeyRef}
                                data-immediate-focus
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
                            disabled={
                              providerStatus === "testing" ||
                              (modelsStatus !== "loading" &&
                                !apiKey.trim() &&
                                selectedProvider != null &&
                                providerRequiresApiKey(selectedProvider) &&
                                !selectedProvider.has_api_key)
                            }
                            onClick={() =>
                              modelsStatus === "loading"
                                ? cancelProviderTest()
                                : void loadModels()
                            }
                          >
                            {modelsStatus === "loading" ? (
                              <CircleX size={15} aria-hidden />
                            ) : (
                              <ListRestart size={15} aria-hidden />
                            )}
                            {modelsStatus === "loading"
                              ? copy.cancelTest
                              : copy.loadModels}
                          </Button>
                          <span
                            className="onboarding-operation-status"
                            role="status"
                          >
                            {modelsStatus === "loading" && (
                              <>
                                <LoaderCircle
                                  className="is-spinning"
                                  size={14}
                                  aria-hidden
                                />
                                {copy.loadingModels}
                              </>
                            )}
                          </span>
                          <p className="onboarding-model-hint">
                            {copy.modelLoadHint}
                          </p>
                        </div>
                        <div className="onboarding-provider-column">
                          <div
                            className="onboarding-provider-field"
                            ref={modelInputRef}
                          >
                            <span className="onboarding-field-label">
                              <Cpu size={14} aria-hidden />
                              {copy.model}
                            </span>
                            <SelectMenu
                              className="onboarding-provider-select"
                              aria-label={copy.model}
                              value={model}
                              options={modelOptions}
                              openRequest={modelOpenRequest}
                              placeholder={copy.chooseModel}
                              search={{
                                placeholder: copy.searchModels,
                                emptyLabel: copy.noMatchingModels,
                              }}
                              menuMaxHeight={360}
                              disabled={
                                modelsStatus !== "ready" ||
                                providerStatus === "testing"
                              }
                              onChange={(value) => {
                                setModel(value);
                                setFocusTest(true);
                                setProviderStatus("idle");
                                verified.current = null;
                                setConnectionIssue(null);
                              }}
                            />
                          </div>

                          <p className="onboarding-test-cost">
                            {copy.testCost}
                          </p>
                          <div className="onboarding-test-action">
                            <button
                              type="button"
                              ref={testButtonRef}
                              className={`onboarding-test-button ${model && modelsStatus === "ready" && providerStatus !== "success" ? "is-next-action" : ""}`}
                              data-status={providerStatus}
                              disabled={
                                providerStatus !== "testing" &&
                                (modelsStatus !== "ready" || !model)
                              }
                              onClick={() =>
                                providerStatus === "testing"
                                  ? cancelProviderTest()
                                  : void testProvider()
                              }
                            >
                              {providerStatus === "testing" ? (
                                <CircleX size={17} aria-hidden />
                              ) : (
                                <Zap size={17} aria-hidden />
                              )}
                              <span className="onboarding-action-label">
                                <span
                                  className="onboarding-action-label-reserve"
                                  aria-hidden
                                >
                                  {copy.cancelTest}
                                </span>
                                <span>
                                  {providerStatus === "testing"
                                    ? copy.cancelTest
                                    : providerStatus === "success"
                                      ? copy.retest
                                      : providerStatus === "error"
                                        ? copy.retry
                                        : copy.test}
                                </span>
                              </span>
                            </button>
                            <span
                              className="onboarding-operation-status"
                              role="status"
                            >
                              {providerStatus === "testing" ? (
                                <>
                                  <LoaderCircle
                                    className="is-spinning"
                                    size={14}
                                    aria-hidden
                                  />
                                  {copy.testing}
                                </>
                              ) : providerStatus === "success" ? (
                                <>
                                  <Check size={15} aria-hidden />
                                  {providerMessage}
                                </>
                              ) : null}
                            </span>
                          </div>
                          {connectionIssue && (
                            <ConnectionIssue
                              kind={connectionIssue}
                              locale={locale}
                              officialConsoleUrl={
                                selectedProvider?.official_key_url
                              }
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
                                  ["quota", "rate_limit"].includes(
                                    connectionIssue,
                                  )
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
                          {providerMessage && providerStatus !== "success" ? (
                            <p
                              className="onboarding-status"
                              data-status={providerStatus}
                              role={
                                providerStatus === "error" ? "alert" : "status"
                              }
                            >
                              {providerMessage}
                            </p>
                          ) : null}
                        </div>
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
                                invoke<ProvidersStateDto>(
                                  "get_providers_state",
                                ),
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

                    <div className="onboarding-split onboarding-workspace-layout">
                      <div className="onboarding-column">
                        <button
                          type="button"
                          className="onboarding-folder-picker"
                          disabled={finishing}
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
                            <small title={effectiveWorkspacePath}>
                              {displayedWorkspacePath ||
                                (workspaceLoadError
                                  ? copy.workspaceLoadFailed
                                  : copy.workspaceLoading)}
                            </small>
                          </span>
                          <span className="onboarding-folder-action">
                            {workspacePath
                              ? copy.changeFolder
                              : copy.chooseFolder}
                            <ChevronRight size={14} aria-hidden />
                          </span>
                        </button>
                        {effectiveWorkspacePath && (
                          <div
                            className="onboarding-workspace-check"
                            data-status={workspaceReady ? "ready" : "pending"}
                            role="status"
                          >
                            <WorkspaceCheckIcon size={15} aria-hidden />
                            <span>
                              {previewProviders
                                ? copy.workspaceCheckDemo
                                : workspaceCheck?.path !==
                                    effectiveWorkspacePath
                                  ? copy.workspaceChecking
                                  : copy.workspaceChecks[workspaceCheck.status]}
                            </span>
                            {!previewProviders &&
                              !workspaceReady &&
                              workspaceCheck?.path ===
                                effectiveWorkspacePath && (
                                <Button
                                  size="sm"
                                  onClick={() =>
                                    setWorkspaceCheckRetry((value) => value + 1)
                                  }
                                >
                                  <RefreshCw size={14} aria-hidden />
                                  {copy.workspaceCheckRetry}
                                </Button>
                              )}
                          </div>
                        )}
                        <p className="onboarding-workspace-confirm">
                          {copy.workspaceConfirm}
                        </p>
                        {workspaceLoadError && !workspacePath && (
                          <div
                            className="onboarding-workspace-error"
                            role="alert"
                          >
                            <span>{copy.workspaceLoadFailed}</span>
                            <Button
                              size="sm"
                              onClick={() =>
                                setConfigRetry((value) => value + 1)
                              }
                            >
                              <RefreshCw size={14} aria-hidden />
                              {copy.workspaceRetry}
                            </Button>
                          </div>
                        )}
                        {workspacePath && (
                          <Button
                            variant="ghost"
                            size="sm"
                            onClick={() => setWorkspacePath("")}
                            disabled={finishing}
                          >
                            <RotateCcw size={14} aria-hidden />
                            {copy.defaultWorkspace}
                          </Button>
                        )}
                      </div>
                      <div className="onboarding-column">
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
                              disabled={finishing}
                              name="onboarding-permission"
                              checked={permissionPreset === "ask_for_approval"}
                              onChange={() =>
                                setPermissionPreset("ask_for_approval")
                              }
                            />
                            <span>
                              <strong className="onboarding-inline-label">
                                <ShieldQuestion size={16} aria-hidden />
                                {copy.ask}
                              </strong>
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
                              disabled={finishing}
                              name="onboarding-permission"
                              checked={permissionPreset === "approve_for_me"}
                              onChange={() =>
                                setPermissionPreset("approve_for_me")
                              }
                            />
                            <span>
                              <strong className="onboarding-inline-label">
                                <Zap size={16} aria-hidden />
                                {copy.approve}
                              </strong>
                              <small>{copy.approveSub}</small>
                            </span>
                            <Check size={17} aria-hidden />
                          </label>
                        </fieldset>
                        <p className="onboarding-network-note">
                          <ShieldCheck size={15} aria-hidden />
                          {copy.networkNote}
                        </p>
                      </div>
                    </div>
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
                    <div className="onboarding-complete-heading">
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
                    </div>

                    {healthSummary && (
                      <div className="onboarding-health-grid" role="status">
                        <div>
                          <Bot size={17} aria-hidden />
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
                          {healthSummary.modelStatus === "verified" ? (
                            <CircleCheck size={17} aria-hidden />
                          ) : (
                            <Info size={17} aria-hidden />
                          )}
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
                          <FolderOpen size={17} aria-hidden />
                          <span>
                            <small>{copy.healthWorkspace}</small>
                            <strong>{healthSummary.workspace}</strong>
                            <small>{healthSummary.workspacePath}</small>
                          </span>
                        </div>
                        <div
                          data-status={
                            healthSummary.sandboxAvailable ? "ok" : "pending"
                          }
                        >
                          {healthSummary.sandboxAvailable ? (
                            <ShieldCheck size={17} aria-hidden />
                          ) : (
                            <ShieldAlert size={17} aria-hidden />
                          )}
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
                    {demoDraft !== null && (
                      <div className="onboarding-demo-composer">
                        <label className="onboarding-task-input">
                          <span className="onboarding-inline-label">
                            <MessageSquare size={16} aria-hidden />
                            {locale === "zh"
                              ? "聊天输入框（演示）"
                              : "Chat input (demo)"}
                          </span>
                          <textarea
                            ref={demoInputRef}
                            value={demoDraft}
                            rows={5}
                            onChange={(event) =>
                              setDemoDraft(event.target.value)
                            }
                          />
                        </label>
                        <p className="onboarding-task-privacy" role="status">
                          {locale === "zh"
                            ? "草稿已填入，可直接编辑。此预览不会保存或发送；APP 中会打开真实聊天输入框。"
                            : "Draft ready to edit. This preview never saves or sends; the app opens the real chat input."}
                        </p>
                      </div>
                    )}
                    <Button
                      className="onboarding-enter-button"
                      variant="ghost"
                      size="sm"
                      onClick={() => enterAstro()}
                    >
                      <MessageSquare size={15} aria-hidden />
                      {copy.enter}
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
                          goTo(
                            step === "workspace" ? "provider" : "personalize",
                          )
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
                          !workspaceReady ||
                          (!previewProviders &&
                            (providerStatus !== "success" || !verified.current))
                        }
                        busyLabel={copy.finishing}
                        onClick={() => void finish()}
                      >
                        {copy.finish}
                        <ChevronRight size={16} aria-hidden />
                      </Button>
                    ) : (
                      <Button
                        ref={continueButtonRef}
                        variant="primary"
                        disabled={
                          step === "provider" && providerStatus !== "success"
                        }
                        onClick={() =>
                          goTo(
                            step === "personalize" ? "provider" : "workspace",
                          )
                        }
                      >
                        {copy.continue}
                        <ChevronRight size={16} aria-hidden />
                      </Button>
                    )}
                  </footer>
                ) : null}
              </div>
            </OnboardingScene>
          )}
        </AnimatePresence>
      </div>
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
        <div
          className="onboarding-app-aperture"
          data-arriving={state === "entering"}
          style={
            {
              "--portal-duration": `${ONBOARDING_WARP_MS.app}ms`,
            } as CSSProperties
          }
          key="app"
        >
          <div
            className="onboarding-app-host"
            data-arriving={state === "entering"}
          >
            <InterfaceTourReady.Provider value={state === "ready"}>
              {children}
            </InterfaceTourReady.Provider>
          </div>
        </div>
      )}
    </>
  );
}
