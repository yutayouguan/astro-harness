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
  ProvidersStateDto,
} from "../../types";
import {
  ONBOARDING_RESET_EVENT,
  inferProjectName,
  normalizeOnboardingStep,
  providerConfigInput,
  providerIsReady,
  providerRequiresApiKey,
  type OnboardingStateDto,
  type OnboardingStep,
} from "../../lib/ui/onboarding";
import { WelcomeLogoEffect } from "../chat/WelcomeLogoEffect";
import { ProviderBrandIcon } from "../icons/ProviderIcons";
import { Button } from "../ui";

const isTauri = () =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

const STEP_ORDER: OnboardingStep[] = ["personalize", "provider", "workspace"];

const COPY = {
  zh: {
    skipIntro: "跳过动画",
    hello: "你好，我是 Astro",
    introSub: "你的本地 AI 工作站，正在苏醒。",
    personalizeEyebrow: "01 · 个性化",
    personalizeTitle: "先让这里更像你的工作空间",
    personalizeSub: "语言和外观会立即生效，之后也可以随时在设置中修改。",
    language: "界面语言",
    theme: "外观主题",
    chinese: "中文",
    english: "English",
    light: "浅色",
    dark: "深色",
    auto: "跟随系统",
    providerEyebrow: "02 · 连接模型",
    providerTitle: "为 Astro 接入思考能力",
    providerSub:
      "选择模型服务并完成一次真实连接测试。密钥只会交给系统安全存储。",
    provider: "模型服务",
    apiKey: "API Key",
    apiKeyStored: "已安全配置，可直接测试",
    apiKeyPlaceholder: "输入 API Key",
    model: "默认模型",
    endpoint: "服务地址",
    advanced: "高级连接设置",
    test: "保存并测试连接",
    testing: "正在验证连接",
    testSuccess: "连接成功",
    noProviders: "没有可用的 Responses API 模型服务。请检查安装配置。",
    providerRequired: "请先填写 API Key。",
    modelRequired: "请填写模型名称。",
    workspaceEyebrow: "03 · 工作空间与安全",
    workspaceTitle: "决定 Astro 从哪里开始工作",
    workspaceSub:
      "可以关联一个代码或资料目录；未选择时会使用 Astro 默认工作空间。",
    chooseFolder: "选择工作目录",
    changeFolder: "更换目录",
    defaultWorkspace: "使用默认工作空间",
    permission: "执行权限",
    ask: "执行前询问",
    askSub: "推荐。涉及写入或高风险操作时先征得你的同意。",
    approve: "自动处理常规操作",
    approveSub: "减少打断，高风险操作仍由安全策略约束。",
    networkNote: "网络默认可用；本机、私网和云元数据地址仍受 SSRF 防护。",
    back: "返回",
    continue: "继续",
    finish: "完成设置",
    finishing: "正在完成初始化",
    completeTitle: "一切准备就绪",
    completeSub: "Astro 已连接模型，并准备好在你的工作空间中开始。",
    enter: "进入 Astro",
    retry: "重试",
    loadError: "无法读取初始化状态，已直接进入 Astro。",
  },
  en: {
    skipIntro: "Skip animation",
    hello: "Hello, I'm Astro",
    introSub: "Your local AI workstation is waking up.",
    personalizeEyebrow: "01 · Personalize",
    personalizeTitle: "Make this workspace feel like yours",
    personalizeSub:
      "Language and appearance update instantly and remain editable in Settings.",
    language: "Interface language",
    theme: "Appearance",
    chinese: "中文",
    english: "English",
    light: "Light",
    dark: "Dark",
    auto: "System",
    providerEyebrow: "02 · Connect a model",
    providerTitle: "Give Astro its reasoning engine",
    providerSub:
      "Choose a provider and complete a real connection check. Keys go only to secure system storage.",
    provider: "Model provider",
    apiKey: "API Key",
    apiKeyStored: "Securely configured and ready to test",
    apiKeyPlaceholder: "Enter API Key",
    model: "Default model",
    endpoint: "Endpoint",
    advanced: "Advanced connection settings",
    test: "Save and test connection",
    testing: "Verifying connection",
    testSuccess: "Connection successful",
    noProviders:
      "No Responses API provider is available. Check the installation configuration.",
    providerRequired: "Enter an API Key first.",
    modelRequired: "Enter a model name.",
    workspaceEyebrow: "03 · Workspace & safety",
    workspaceTitle: "Choose where Astro starts working",
    workspaceSub:
      "Attach a code or document folder, or continue with Astro's default workspace.",
    chooseFolder: "Choose workspace",
    changeFolder: "Change folder",
    defaultWorkspace: "Use default workspace",
    permission: "Execution permissions",
    ask: "Ask before acting",
    askSub: "Recommended. Astro asks before writes or higher-risk actions.",
    approve: "Handle routine actions",
    approveSub:
      "Fewer interruptions; higher-risk actions remain policy controlled.",
    networkNote:
      "Network access is available by default; local, private, and cloud metadata targets remain SSRF-protected.",
    back: "Back",
    continue: "Continue",
    finish: "Finish setup",
    finishing: "Finishing setup",
    completeTitle: "Everything is ready",
    completeSub: "Astro is connected and ready to begin in your workspace.",
    enter: "Enter Astro",
    retry: "Retry",
    loadError: "Setup state could not be loaded, so Astro opened directly.",
  },
} as const;

type PermissionPreset = "ask_for_approval" | "approve_for_me";
type ProviderStatus = "idle" | "testing" | "success" | "error";

type FirstRunOnboardingProps = {
  initialStep?: OnboardingStep;
  onComplete: () => void;
  previewProviders?: ProviderDto[];
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

function OnboardingLogo({ compact = false }: { compact?: boolean }) {
  return (
    <div
      className={`onboarding-logo ${compact ? "is-compact" : ""}`}
      aria-hidden
    >
      <span className="onboarding-logo-halo" />
      <span className="chat-welcome-mark onboarding-logo-mark">
        <span className="chat-welcome-mark-glow" />
        <span className="chat-welcome-illust">
          <WelcomeLogoEffect />
        </span>
      </span>
    </div>
  );
}

export function FirstRunOnboarding({
  initialStep = "intro",
  onComplete,
  previewProviders,
}: FirstRunOnboardingProps) {
  const { locale, setLocale } = useI18n();
  const { mode, setMode } = useTheme();
  const reducedMotion = useReducedMotion() ?? false;
  const copy = COPY[locale];
  const [step, setStep] = useState<OnboardingStep>(initialStep);
  const [direction, setDirection] = useState(1);
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
  const [selectedProviderId, setSelectedProviderId] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [model, setModel] = useState("");
  const [endpoint, setEndpoint] = useState("");
  const [providerStatus, setProviderStatus] = useState<ProviderStatus>("idle");
  const [providerMessage, setProviderMessage] = useState("");
  const [workspacePath, setWorkspacePath] = useState("");
  const [permissionPreset, setPermissionPreset] =
    useState<PermissionPreset>("ask_for_approval");
  const [finishing, setFinishing] = useState(false);
  const [finishError, setFinishError] = useState("");
  const headingRef = useRef<HTMLHeadingElement>(null);

  const providers = useMemo(
    () =>
      (providersState?.providers ?? []).filter(
        (provider) => provider.supports_responses_api === true,
      ),
    [providersState],
  );
  const selectedProvider =
    providers.find((provider) => provider.id === selectedProviderId) ?? null;

  const persistStep = useCallback((next: OnboardingStep) => {
    if (!isTauri() || next === "intro" || next === "complete") return;
    void invoke("save_onboarding_progress", { step: next }).catch((error) => {
      console.warn("onboarding progress save failed", error);
    });
  }, []);

  const goTo = useCallback(
    (next: OnboardingStep) => {
      setDirection(stepIndex(next) >= stepIndex(step) ? 1 : -1);
      setStep(next);
      persistStep(next);
    },
    [persistStep, step],
  );

  useEffect(() => {
    if (step !== "intro") return;
    const timer = window.setTimeout(
      () => goTo("personalize"),
      reducedMotion ? 450 : 2200,
    );
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") goTo("personalize");
    };
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.clearTimeout(timer);
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [goTo, reducedMotion, step]);

  useEffect(() => {
    if (step === "intro" || step === "complete") return;
    window.requestAnimationFrame(() => headingRef.current?.focus());
  }, [step]);

  useEffect(() => {
    if (step !== "complete") return;
    const timer = window.setTimeout(onComplete, reducedMotion ? 250 : 950);
    return () => window.clearTimeout(timer);
  }, [onComplete, reducedMotion, step]);

  useEffect(() => {
    if (previewProviders || !isTauri()) return;
    let disposed = false;
    void invoke<ProvidersStateDto>("get_providers_state")
      .then((state) => {
        if (!disposed) setProvidersState(state);
      })
      .catch((error) => {
        if (!disposed) setProviderMessage(String(error));
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
      setModel(preferred.model);
      setEndpoint(preferred.endpoint);
    }
  }, [providers, providersState?.active_provider_id, selectedProviderId]);

  const selectProvider = (id: string) => {
    const provider = providers.find((item) => item.id === id);
    if (!provider) return;
    setSelectedProviderId(provider.id);
    setModel(provider.model);
    setEndpoint(provider.endpoint);
    setApiKey("");
    setProviderStatus("idle");
    setProviderMessage("");
  };

  const testProvider = async () => {
    if (!selectedProvider) return;
    if (!model.trim()) {
      setProviderStatus("error");
      setProviderMessage(copy.modelRequired);
      return;
    }
    if (
      providerRequiresApiKey(selectedProvider) &&
      !selectedProvider.has_api_key &&
      !apiKey.trim()
    ) {
      setProviderStatus("error");
      setProviderMessage(copy.providerRequired);
      return;
    }
    setProviderStatus("testing");
    setProviderMessage("");
    try {
      if (previewProviders || !isTauri()) {
        await new Promise((resolve) => window.setTimeout(resolve, 420));
        setProviderStatus("success");
        setProviderMessage(copy.testSuccess);
        return;
      }
      const next = await invoke<ProvidersStateDto>("save_provider", {
        provider: {
          ...providerConfigInput(selectedProvider, model),
          endpoint: endpoint.trim() || selectedProvider.endpoint,
        },
      });
      let current = next;
      if (apiKey.trim()) {
        current = await invoke<ProvidersStateDto>("set_provider_api_key", {
          id: selectedProvider.id,
          apiKey: apiKey.trim(),
        });
        setApiKey("");
      }
      const result = await invoke<ProviderTestResult>("test_provider", {
        id: selectedProvider.id,
        model: model.trim(),
      });
      if (!result.ok) throw new Error(result.message);
      current = await invoke<ProvidersStateDto>("set_active_provider_model", {
        id: selectedProvider.id,
        model: result.model,
      });
      setProvidersState(current);
      setProviderStatus("success");
      setProviderMessage(
        `${copy.testSuccess} · ${result.model} · ${result.latency_ms} ms`,
      );
    } catch (error) {
      setProviderStatus("error");
      setProviderMessage(
        error instanceof Error ? error.message : String(error),
      );
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
    setFinishing(true);
    setFinishError("");
    try {
      if (!previewProviders && isTauri()) {
        await invoke("set_permission_preset", {
          preset: permissionPreset,
          confirmed: false,
        });
        if (workspacePath) {
          const projects = await invoke<ProjectDto[]>("list_projects");
          const alreadyExists = projects.some((project) =>
            project.roots.includes(workspacePath),
          );
          if (!alreadyExists) {
            await invoke<ProjectDto>("create_project", {
              name: inferProjectName(workspacePath),
              roots: [workspacePath],
            });
          }
        }
        await invoke<OnboardingStateDto>("complete_onboarding");
      }
      setStep("complete");
    } catch (error) {
      setFinishError(error instanceof Error ? error.message : String(error));
      setFinishing(false);
    }
  };

  const variants = slideVariants(direction, reducedMotion);
  const currentIndex = Math.max(0, stepIndex(step));

  return (
    <main className="onboarding-root" data-step={step}>
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
            <OnboardingLogo />
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
              <div className="onboarding-brand">
                <OnboardingLogo compact />
                <span>Astro Agent</span>
              </div>
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
              {step === "personalize" ? (
                <>
                  <div className="onboarding-heading">
                    <span className="onboarding-eyebrow">
                      <Sparkles size={14} aria-hidden />
                      {copy.personalizeEyebrow}
                    </span>
                    <h1 ref={headingRef} tabIndex={-1}>
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
                </>
              ) : null}

              {step === "provider" ? (
                <>
                  <div className="onboarding-heading">
                    <span className="onboarding-eyebrow">
                      <KeyRound size={14} aria-hidden />
                      {copy.providerEyebrow}
                    </span>
                    <h1 ref={headingRef} tabIndex={-1}>
                      {copy.providerTitle}
                    </h1>
                    <p>{copy.providerSub}</p>
                  </div>

                  {providers.length > 0 ? (
                    <div className="onboarding-provider-form">
                      <label>
                        <span>{copy.provider}</span>
                        <div className="onboarding-select-wrap">
                          {selectedProvider ? (
                            <ProviderBrandIcon kind={selectedProvider.kind} />
                          ) : null}
                          <select
                            value={selectedProviderId}
                            onChange={(event) =>
                              selectProvider(event.target.value)
                            }
                          >
                            {providers.map((provider) => (
                              <option key={provider.id} value={provider.id}>
                                {provider.display_name}
                              </option>
                            ))}
                          </select>
                        </div>
                      </label>

                      {selectedProvider &&
                      providerRequiresApiKey(selectedProvider) ? (
                        <label>
                          <span>{copy.apiKey}</span>
                          <input
                            type="password"
                            value={apiKey}
                            placeholder={
                              selectedProvider.has_api_key
                                ? copy.apiKeyStored
                                : copy.apiKeyPlaceholder
                            }
                            autoComplete="off"
                            spellCheck={false}
                            onChange={(event) => {
                              setApiKey(event.target.value);
                              setProviderStatus("idle");
                            }}
                          />
                        </label>
                      ) : null}

                      <label>
                        <span>{copy.model}</span>
                        <input
                          value={model}
                          spellCheck={false}
                          onChange={(event) => {
                            setModel(event.target.value);
                            setProviderStatus("idle");
                          }}
                        />
                      </label>

                      <details className="onboarding-advanced">
                        <summary>{copy.advanced}</summary>
                        <label>
                          <span>{copy.endpoint}</span>
                          <input
                            value={endpoint}
                            spellCheck={false}
                            onChange={(event) => {
                              setEndpoint(event.target.value);
                              setProviderStatus("idle");
                            }}
                          />
                        </label>
                      </details>

                      <button
                        type="button"
                        className="onboarding-test-button"
                        data-status={providerStatus}
                        disabled={providerStatus === "testing"}
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
                    <p className="onboarding-empty" role="alert">
                      {providerMessage || copy.noProviders}
                    </p>
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
                    <h1 ref={headingRef} tabIndex={-1}>
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
                  <h1 ref={headingRef} tabIndex={-1}>
                    {copy.completeTitle}
                  </h1>
                  <p>{copy.completeSub}</p>
                  <Button variant="primary" size="lg" onClick={onComplete}>
                    {copy.enter}
                    <ChevronRight size={17} aria-hidden />
                  </Button>
                </div>
              ) : null}

              {step !== "complete" ? (
                <footer className="onboarding-actions">
                  {step !== "personalize" ? (
                    <Button
                      variant="ghost"
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
  const [state, setState] = useState<"loading" | "visible" | "ready">(
    isTauri() ? "loading" : "ready",
  );
  const [initialStep, setInitialStep] = useState<OnboardingStep>("intro");

  const showOnboarding = useCallback((step: OnboardingStep = "intro") => {
    setInitialStep(step);
    setState("visible");
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    let disposed = false;
    void invoke<OnboardingStateDto>("get_onboarding_state")
      .then((next) => {
        if (disposed) return;
        if (next.should_show)
          showOnboarding(normalizeOnboardingStep(next.step));
        else setState("ready");
      })
      .catch((error) => {
        console.warn(COPY.zh.loadError, error);
        if (!disposed) setState("ready");
      });
    return () => {
      disposed = true;
    };
  }, [showOnboarding]);

  useEffect(() => {
    const handleReset = () => showOnboarding("intro");
    window.addEventListener(ONBOARDING_RESET_EVENT, handleReset);
    return () =>
      window.removeEventListener(ONBOARDING_RESET_EVENT, handleReset);
  }, [showOnboarding]);

  if (state === "loading") {
    return (
      <main
        className="onboarding-root onboarding-root--loading"
        aria-busy="true"
      >
        <OnboardingLogo compact />
      </main>
    );
  }
  if (state === "visible") {
    return (
      <FirstRunOnboarding
        initialStep={initialStep}
        onComplete={() => setState("ready")}
      />
    );
  }
  return children;
}
