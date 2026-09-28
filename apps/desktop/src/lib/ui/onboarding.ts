import type { ProviderDto } from "../../types";

export const ONBOARDING_VERSION = 1;
export const ONBOARDING_RESET_EVENT = "astro:onboarding-reset";
export const ONBOARDING_STARTER_PROMPT_KEY =
  "astro.onboarding.starterPrompt.v1";
let pendingStarterPrompt: string | null = null;

export const ONBOARDING_STEPS = [
  "intro",
  "personalize",
  "provider",
  "workspace",
  "complete",
] as const;

export type OnboardingStep = (typeof ONBOARDING_STEPS)[number];

export type OnboardingStateDto = {
  version: number;
  step: string;
  completed: boolean;
  should_show: boolean;
  inferred_existing_install: boolean;
  updated_at: string | null;
  draft?: OnboardingDraft;
  first_meeting?: {
    status: "pending" | "deferred" | "started";
    session_id: string | null;
  } | null;
};

export function normalizeOnboardingStep(step: string): OnboardingStep {
  return ONBOARDING_STEPS.includes(step as OnboardingStep)
    ? (step as OnboardingStep)
    : "intro";
}

export {
  providerIsReady,
  providerRequiresApiKey,
} from "../providers/providerReadiness.ts";

export function providerConfigInput(provider: ProviderDto, model: string) {
  return {
    id: provider.id,
    kind: provider.kind,
    display_name: provider.display_name,
    endpoint: provider.endpoint,
    model: model.trim() || provider.model,
    enabled: true,
    fallback: provider.fallback ?? [],
    image_model: provider.image_model ?? "",
    video_model: provider.video_model ?? "",
    tts_model: provider.tts_model ?? "",
    music_model: provider.music_model ?? "",
    vision_model: provider.vision_model ?? "",
    embedding_model: provider.embedding_model ?? "",
  };
}

export function inferProjectName(path: string): string {
  const segments = path.split(/[\\/]/).filter(Boolean);
  return segments[segments.length - 1] ?? "Workspace";
}

export function storeOnboardingStarterPrompt(prompt: string): void {
  const value = prompt.trim();
  if (!value || typeof window === "undefined") return;
  pendingStarterPrompt = value;
  try {
    window.sessionStorage.setItem(ONBOARDING_STARTER_PROMPT_KEY, value);
  } catch {
    // Session storage may be unavailable in restricted WebViews.
  }
}

export function takeOnboardingStarterPrompt(): string | null {
  if (typeof window === "undefined") return null;
  const pending = pendingStarterPrompt;
  pendingStarterPrompt = null;
  try {
    const value = window.sessionStorage.getItem(ONBOARDING_STARTER_PROMPT_KEY);
    window.sessionStorage.removeItem(ONBOARDING_STARTER_PROMPT_KEY);
    return pending || value?.trim() || null;
  } catch {
    return pending;
  }
}

export type OnboardingDraft = {
  agent_name: string;
  provider_id: string;
  model: string;
  endpoint: string;
  workspace_path: string;
  permission_preset: "ask_for_approval" | "approve_for_me";
  pet_enabled?: boolean | null;
};
export const EMPTY_ONBOARDING_DRAFT: OnboardingDraft = {
  agent_name: "Astro",
  provider_id: "",
  model: "",
  endpoint: "",
  workspace_path: "",
  permission_preset: "ask_for_approval",
};

/** Invalid endpoints and URLs containing credentials must not enter a setup draft. */
export function persistableOnboardingEndpoint(value: string): string {
  try {
    const url = new URL(value.trim());
    if (
      !["http:", "https:"].includes(url.protocol) ||
      url.username ||
      url.password
    )
      return "";
    for (const key of url.searchParams.keys()) {
      if (/key|token|secret|password|authorization/i.test(key)) return "";
    }
    return value.trim();
  } catch {
    return "";
  }
}

export type ConnectionIssueKind =
  | "verification"
  | "credentials"
  | "quota"
  | "model"
  | "timeout"
  | "network"
  | "rate_limit"
  | "unknown";
/** Never display raw provider errors: they can contain echoed request headers or keys. */
export function classifyConnectionIssue(error: unknown): ConnectionIssueKind {
  const message = String(
    error instanceof Error ? error.message : error,
  ).toLowerCase();
  if (/onboarding_verification_required/.test(message)) return "verification";
  if (
    /insufficient_quota|quota.exceed|billing|credit|balance|余额|欠费|额度不足|402/.test(
      message,
    )
  )
    return "quota";
  if (
    /401|403|unauthori|authentication|invalid.api.key|密钥|鉴权/.test(message)
  )
    return "credentials";
  if (
    /model.not.found|model_not_found|404|deploymentnotfound|模型不存在|unknown.model/.test(
      message,
    )
  )
    return "model";
  if (/timeout|timed.out|超时|aborterror/.test(message)) return "timeout";
  if (/429|rate.limit|too.many.requests|限流/.test(message))
    return "rate_limit";
  if (/network|fetch|connection|dns|econn|offline|网络|连接失败/.test(message))
    return "network";
  return "unknown";
}

/**
 * 服务商官方密钥链接：只接受 https 且不带凭据的地址。
 * 元数据被污染时宁可不给入口，也不打开非 https 或带用户名密码的地址。
 */
export function officialKeyUrl(value?: string | null): string | null {
  if (!value) return null;
  try {
    const url = new URL(value);
    if (url.protocol !== "https:" || url.username || url.password) return null;
    return url.href;
  } catch {
    return null;
  }
}

/** 原生外壳走系统浏览器；浏览器 / Storybook 预览退化为新标签页。失败返回 false。 */
export async function openExternalUrl(url: string): Promise<boolean> {
  try {
    if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
      const { openUrl } = await import("@tauri-apps/plugin-opener");
      await openUrl(url);
    } else {
      window.open(url, "_blank", "noopener,noreferrer");
    }
    return true;
  } catch {
    return false;
  }
}

export function withDeadline<T>(promise: Promise<T>, ms = 20000): Promise<T> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("timeout")), ms);
    promise.then(
      (value) => {
        clearTimeout(timer);
        resolve(value);
      },
      (error) => {
        clearTimeout(timer);
        reject(error);
      },
    );
  });
}

/** Serialize progress and finalization so a delayed save cannot reopen completed setup. */
export function createOnboardingWriteQueue() {
  let pending: Promise<unknown> = Promise.resolve();
  return {
    run<T>(write: () => Promise<T>): Promise<T> {
      const result = pending.catch(() => undefined).then(write);
      pending = result;
      return result;
    },
    flush: () => pending,
  };
}

/** A saved workspace step is not proof of a live connection after restarting. */
export function resumeOnboardingStep(step: string): OnboardingStep {
  return step === "workspace" ? "provider" : normalizeOnboardingStep(step);
}
