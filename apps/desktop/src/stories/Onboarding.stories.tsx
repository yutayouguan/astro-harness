import type { Meta, StoryObj } from "@storybook/react-vite";
import { FirstRunOnboarding } from "../components/onboarding/OnboardingGate";
import { LocaleProvider } from "../i18n/LocaleContext";
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

function Preview({
  step,
}: {
  step: "intro" | "personalize" | "provider" | "workspace" | "complete";
}) {
  return (
    <ThemeProvider>
      <MorphiconProvider>
        <LocaleProvider>
          <FirstRunOnboarding
            initialStep={step}
            previewProviders={providers}
            disableIntroAdvance={step === "intro"}
            onComplete={() => undefined}
          />
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
