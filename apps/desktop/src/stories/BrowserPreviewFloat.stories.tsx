import type { Meta, StoryObj } from "@storybook/react";
import BrowserPreviewFloat from "../components/chat/BrowserPreviewFloat";
import { LocaleProvider } from "../i18n/LocaleContext";

const screenshot = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(`
  <svg xmlns="http://www.w3.org/2000/svg" width="960" height="600">
    <rect width="960" height="600" fill="#f7f8fb"/>
    <rect x="0" y="0" width="960" height="54" fill="#ffffff"/>
    <circle cx="30" cy="27" r="8" fill="#7357e8"/>
    <text x="52" y="34" font-family="system-ui" font-size="20" font-weight="650" fill="#202533">Astro Preview</text>
    <rect x="120" y="120" width="720" height="330" rx="28" fill="#ffffff" stroke="#e5e7ef"/>
    <text x="170" y="205" font-family="system-ui" font-size="38" font-weight="700" fill="#242938">Build with confidence</text>
    <text x="170" y="250" font-family="system-ui" font-size="18" fill="#747b8d">The agent can inspect, click, modify, and verify this page.</text>
    <rect x="170" y="310" width="150" height="48" rx="14" fill="#7058e8"/>
    <text x="205" y="341" font-family="system-ui" font-size="16" font-weight="650" fill="#ffffff">Get started</text>
  </svg>
`)}`;

const meta = {
  title: "Chat/BrowserPreviewFloat",
  component: BrowserPreviewFloat,
  decorators: [
    (Story) => (
      <LocaleProvider>
        <div
          style={{
            position: "relative",
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
      url: "http://127.0.0.1:1420/dashboard",
      title: "Astro Preview",
      screenshotPath: screenshot,
      status: "connected",
      action: "snapshot",
      updatedAt: Date.now(),
    },
    onClose: () => undefined,
  },
} satisfies Meta<typeof BrowserPreviewFloat>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Connected: Story = {};

export const Disconnected: Story = {
  args: {
    preview: {
      sessionId: "story-session",
      url: "http://127.0.0.1:1420/dashboard",
      title: "Astro Preview",
      screenshotPath: screenshot,
      status: "disconnected",
      action: "snapshot",
      updatedAt: Date.now(),
    },
  },
};
