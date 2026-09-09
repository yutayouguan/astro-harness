import type { Meta, StoryObj } from "@storybook/react-vite";
import { useCallback, useState, type CSSProperties } from "react";
import { ONBOARDING_WARP_MS } from "../lib/ui/onboardingMotion";
import { FirstRunOnboarding } from "../components/onboarding/OnboardingGate";
import { AstroLogoMark } from "../components/icons/AstroLogoMark";
import { Button } from "../components/ui";
import { LocaleProvider, useI18n } from "../i18n/LocaleContext";
import { ThemeProvider } from "../hooks/app/useTheme";
import { MorphiconProvider } from "../hooks/app/useMorphicons";
import type { ProviderDto } from "../types";

const providers: ProviderDto[] = [
  {
    id: "openai",
    kind: "openai",
    display_name: "OpenAI",
    endpoint: "https://api.openai.com/v1",
    model: "gpt-5.6",
    enabled: true,
    has_api_key: false,
    key_source: "none",
    env_key_name: "OPENAI_API_KEY",
    backend_id: "openai",
    supports_responses_api: true,
  },
  {
    id: "google",
    kind: "google",
    display_name: "Google Gemini",
    endpoint: "https://generativelanguage.googleapis.com",
    model: "gemini-3.1-pro",
    enabled: true,
    has_api_key: true,
    key_source: "keyring",
    env_key_name: "GEMINI_API_KEY",
    backend_id: "google",
    supports_responses_api: true,
  },
];

type PreviewProps = {
  step: "intro" | "personalize" | "provider" | "workspace" | "complete";
  journey?: boolean;
};

/** A clearly labelled local destination; never mounts native transports or sends prompts. */
function JourneyPreview({ step, journey = false }: PreviewProps) {
  const { locale } = useI18n();
  const [phase, setPhase] = useState<"setup" | "entering" | "ready">("setup");
  const [prompt, setPrompt] = useState("");
  const arrive = useCallback(() => setPhase("ready"), []);
  const enter = useCallback((draft: string) => {
    setPrompt(draft);
    setPhase("entering");
  }, []);
  const zh = locale === "zh";
  return (
    <>
      {phase !== "ready" && (
        <FirstRunOnboarding
          initialStep={step}
          previewProviders={providers}
          disableIntroAdvance={step === "intro" && !journey}
          handoff={phase === "entering"}
          onHandoffComplete={arrive}
          onPreviewEnter={journey ? enter : undefined}
          onComplete={() => undefined}
        />
      )}
      {phase !== "setup" && (
        <div
          className="onboarding-app-aperture"
          data-arriving={phase === "entering"}
          style={
            {
              "--portal-duration": `${ONBOARDING_WARP_MS.app}ms`,
            } as CSSProperties
          }
        >
          <div
            className="onboarding-app-host"
            data-arriving={phase === "entering"}
          >
            <main className="onboarding-arrival-preview">
              <span className="onboarding-demo-label">
                {zh
                  ? "APP 进入动画演示 · 不保存、不发送"
                  : "App arrival preview · nothing saved or sent"}
              </span>
              <div className="onboarding-arrival-preview-content">
                <AstroLogoMark
                  className="onboarding-arrival-preview-logo"
                  data-onboarding-brand-target
                />
                <h1>{zh ? "从这里，开始协作" : "Let's work together"}</h1>
                <p>
                  {zh
                    ? "已抵达你的工作空间。下面的草稿可以继续编辑。"
                    : "Welcome to your workspace. Your draft is ready to edit."}
                </p>
                <label className="onboarding-task-input">
                  {zh ? "聊天输入框（演示）" : "Chat input (demo)"}
                  <textarea
                    value={prompt}
                    onChange={(event) => setPrompt(event.target.value)}
                    rows={5}
                  />
                </label>
                <Button
                  variant="ghost"
                  onClick={() => {
                    setPhase("setup");
                    setPrompt("");
                  }}
                >
                  {zh ? "重新体验" : "Replay journey"}
                </Button>
              </div>
            </main>
          </div>
        </div>
      )}
    </>
  );
}

function Preview(props: PreviewProps) {
  return (
    <ThemeProvider>
      <MorphiconProvider>
        <LocaleProvider>
          <JourneyPreview key={`${props.step}-${props.journey}`} {...props} />
        </LocaleProvider>
      </MorphiconProvider>
    </ThemeProvider>
  );
}

const meta = {
  title: "App/First-run onboarding",
  component: Preview,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof Preview>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Intro: Story = { args: { step: "intro" } };
export const Personalize: Story = { args: { step: "personalize" } };
export const Provider: Story = { args: { step: "provider" } };
export const Workspace: Story = { args: { step: "workspace" } };
export const Complete: Story = { args: { step: "complete" } };
export const Journey: Story = { args: { step: "intro", journey: true } };
export const EnterApp: Story = { args: { step: "complete", journey: true } };
