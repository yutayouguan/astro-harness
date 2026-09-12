import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { Meta, StoryObj } from "@storybook/react-vite";
import type { CSSProperties } from "react";
import ProvidersPanel from "../components/settings/ProvidersPanel";
import { MorphiconProvider } from "../hooks/app/useMorphicons";
import { DialogProvider } from "../hooks/ui/DialogContext";
import { LocaleProvider } from "../i18n/LocaleContext";
import type { ProviderDto, ProvidersStateDto } from "../types";

const provider = (
  id: string,
  kind: string,
  display_name: string,
  model: string,
): ProviderDto => ({
  id,
  kind,
  display_name,
  model,
  endpoint: "https://example.invalid/v1",
  enabled: true,
  has_api_key: true,
  key_source: "env",
  env_key_name: null,
  backend_id: kind,
  supports_responses_api: true,
});

const meta = {
  title: "Settings/ProvidersPanel",
  component: ProvidersPanel,
  beforeEach: (context: { id: string }) => {
    let state: ProvidersStateDto = {
      providers: [
        provider(
          "azure",
          "azure",
          context.id.endsWith("--long-names")
            ? "Azure OpenAI Enterprise Production Workspace"
            : "Azure OpenAI",
          "gpt-6-astra",
        ),
        provider(
          "openai",
          "openai",
          "OpenAI",
          "gpt-6-astra-long-deployment-name-for-overflow",
        ),
        {
          ...provider("deepseek", "deepseek", "DeepSeek", "deepseek-chat"),
          enabled: false,
        },
      ],
      active_provider_id: "azure",
      active_image_provider_id: null,
    };
    mockIPC((command, payload) => {
      if (command === "set_app_menu_locale") return null;
      if (command === "get_providers_state") return structuredClone(state);
      if (command === "get_provider_api_key") return null;
      if (command === "get_cached_provider_models") return null;
      if (command === "list_provider_models")
        return { models: [], latency_ms: 12, source: "fixture" };
      if (command === "reorder_providers") {
        const { ids } = payload as { ids: string[] };
        const byId = new Map(state.providers.map((p) => [p.id, p]));
        state = { ...state, providers: ids.map((id) => byId.get(id)!) };
        return structuredClone(state);
      }
      throw new Error(`Unexpected provider preview command: ${command}`);
    });
    return () => clearMocks();
  },
  decorators: [
    (Story) => (
      <LocaleProvider>
        <MorphiconProvider>
          <DialogProvider>
            <div
              className="settings-content-inline"
              style={
                {
                  height: "100vh",
                  padding: 20,
                  background: "var(--bg)",
                  "--tone": "var(--tone-amber)",
                  "--tone-soft": "var(--tone-amber-soft)",
                } as CSSProperties
              }
            >
              <Story />
            </div>
          </DialogProvider>
        </MorphiconProvider>
      </LocaleProvider>
    ),
  ],
  args: { active: true, tone: "amber" },
} satisfies Meta<typeof ProvidersPanel>;

export default meta;
type Story = StoryObj<typeof meta>;
export const Compact: Story = {};
export const LongNames: Story = {};
