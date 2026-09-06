import type { Meta, StoryObj } from "@storybook/react-vite";
import PluginsPage from "../components/plugins/PluginsPage";
import { ActiveAgentProvider } from "../hooks/app/useActiveAgent";
import { MorphiconProvider } from "../hooks/app/useMorphicons";
import { LocaleProvider } from "../i18n/LocaleContext";

const meta = {
  title: "Pages/PluginsPage",
  component: PluginsPage,
  args: {
    active: false,
    tone: "indigo",
  },
  decorators: [
    (Story) => (
      <LocaleProvider>
        <ActiveAgentProvider>
          <MorphiconProvider>
            <main
              style={{
                boxSizing: "border-box",
                width: "100vw",
                height: "100vh",
                padding: 28,
                background: "var(--shell-bg)",
                color: "var(--ink)",
              }}
            >
              <Story />
            </main>
          </MorphiconProvider>
        </ActiveAgentProvider>
      </LocaleProvider>
    ),
  ],
} satisfies Meta<typeof PluginsPage>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
