import type { Meta, StoryObj } from "@storybook/react";
import BrowserSettingsPanel from "../components/settings/BrowserSettingsPanel";
import { LocaleProvider } from "../i18n/LocaleContext";

const meta = {
  title: "Settings/BrowserSettingsPanel",
  component: BrowserSettingsPanel,
  decorators: [
    (Story) => (
      <LocaleProvider>
        <div
          style={{
            width: "100%",
            height: "100vh",
            minHeight: 720,
            background: "var(--bg)",
          }}
        >
          <Story />
        </div>
      </LocaleProvider>
    ),
  ],
  args: {
    active: true,
    tone: "twilight",
  },
} satisfies Meta<typeof BrowserSettingsPanel>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
