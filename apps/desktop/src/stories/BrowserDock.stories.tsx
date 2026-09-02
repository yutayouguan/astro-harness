import type { Meta, StoryObj } from "@storybook/react";
import BrowserDock from "../components/chat/BrowserDock";
import { LocaleProvider } from "../i18n/LocaleContext";

const screenshot = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(`
  <svg xmlns="http://www.w3.org/2000/svg" width="1280" height="800">
    <rect width="1280" height="800" fill="#f4f6fb"/>
    <rect x="92" y="88" width="1096" height="624" rx="30" fill="#ffffff" stroke="#dfe4ef"/>
    <circle cx="148" cy="142" r="17" fill="#7058e8"/>
    <text x="184" y="151" font-family="system-ui" font-size="27" font-weight="700" fill="#222735">Astro Preview</text>
    <text x="148" y="275" font-family="system-ui" font-size="54" font-weight="750" fill="#202533">Build, inspect, and iterate.</text>
    <text x="148" y="328" font-family="system-ui" font-size="22" fill="#72798b">You and the Agent share this browser session.</text>
    <rect x="148" y="390" width="190" height="58" rx="15" fill="#7058e8"/>
    <text x="198" y="427" font-family="system-ui" font-size="19" font-weight="650" fill="#ffffff">Get started</text>
  </svg>
`)}`;

const meta = {
  title: "Chat/BrowserDock",
  component: BrowserDock,
  decorators: [
    (Story) => (
      <LocaleProvider>
        <div
          style={{
            display: "flex",
            justifyContent: "flex-end",
            width: "100%",
            height: "100vh",
            minHeight: 620,
            background: "var(--bg)",
          }}
        >
          <Story />
        </div>
      </LocaleProvider>
    ),
  ],
  args: {
    preview: {
      sessionId: "story-session",
      url: "http://127.0.0.1:5173",
      title: "Astro Preview",
      screenshotPath: screenshot,
      status: "connected",
      action: "snapshot",
      updatedAt: Date.now(),
      activeTabId: "tab-1",
      tabs: [
        {
          id: "tab-1",
          title: "Astro Preview",
          url: "http://127.0.0.1:5173",
          active: true,
        },
        {
          id: "tab-2",
          title: "Documentation",
          url: "https://example.com/docs",
          active: false,
        },
      ],
      downloads: [
        {
          name: "report.pdf",
          path: "/tmp/report.pdf",
          size: 842_721,
          status: "complete",
          updatedAt: Date.now(),
        },
      ],
    },
    expanded: false,
    onControl: async () => undefined,
    onExpandedChange: () => undefined,
    onClose: () => undefined,
  },
} satisfies Meta<typeof BrowserDock>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Connected: Story = {};

export const Empty: Story = {
  args: {
    preview: null,
  },
};
