import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { Meta, StoryObj } from "@storybook/react-vite";
import ToolsPanel from "../components/settings/ToolsPanel";
import { ActiveAgentProvider } from "../hooks/app/useActiveAgent";
import { MorphiconProvider } from "../hooks/app/useMorphicons";
import { DialogProvider } from "../hooks/ui/DialogContext";
import { LocaleProvider } from "../i18n/LocaleContext";
import type { ModelInfo } from "../types";

const meta = {
  id: "tools-loading",
  title: "Settings/Tool Loading",
  component: ToolsPanel,
  beforeEach: () => {
    localStorage.setItem("astro.tools.viewMode", "detail");
    const settings = () => ({
      modes: JSON.parse(sessionStorage.getItem("qa.toolLoading") ?? "{}"),
      adjustableToolsets: ["browser", "web_search"],
    });
    mockIPC((command, payload) => {
      const args = payload as Record<string, unknown>;
      if (command === "get_config")
        return { active_agent_id: "default", agents: [] };
      if (command === "get_tools_enabled")
        return { browser: true, web_search: true };
      if (command === "get_tool_loading_settings") return settings();
      if (command === "set_tool_loading_mode") {
        if (sessionStorage.getItem("qa.failSave"))
          throw new Error("QA save failed");
        const next = settings();
        if (args.mode === "auto") delete next.modes[String(args.toolset)];
        else next.modes[String(args.toolset)] = args.mode;
        sessionStorage.setItem("qa.toolLoading", JSON.stringify(next.modes));
        return next;
      }
      if (command === "get_tool_catalog")
        return [
          {
            id: "browser",
            name: "astro_browser.open",
            namespace: "astro_browser",
            registeredName: "browser_open",
            description: "Browser automation",
            icon: "globe",
            exposure: "deferred",
            params: [],
            tools: ["astro_browser.open"],
            functions: [
              {
                name: "astro_browser.open",
                namespace: "astro_browser",
                registeredName: "browser_open",
                description: "Open a page",
                icon: "globe",
                params: [],
                exposure: "deferred",
              },
            ],
          },
        ];
      return null;
    });
    return () => clearMocks();
  },
  decorators: [
    (Story) => (
      <LocaleProvider>
        <ActiveAgentProvider>
          <MorphiconProvider>
            <DialogProvider>
              <main
                className="settings-content-inline"
                style={{ height: "100vh", padding: 24 }}
              >
                <Story />
              </main>
            </DialogProvider>
          </MorphiconProvider>
        </ActiveAgentProvider>
      </LocaleProvider>
    ),
  ],
} satisfies Meta<typeof ToolsPanel>;
export default meta;
type Story = StoryObj<typeof meta>;

export const Supported: Story = {
  args: {
    modelInfo: {
      id: "qa",
      profile: { supports_search_tool: true },
    } as ModelInfo,
  },
};
export const Unsupported: Story = {
  args: {
    modelInfo: {
      id: "qa",
      profile: { supports_search_tool: false },
    } as ModelInfo,
  },
};
