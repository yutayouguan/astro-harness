import type { Meta, StoryObj } from "@storybook/react-vite";
import { useEffect, useState, type CSSProperties } from "react";
import { useInAppBrowserLinks } from "../hooks/ui/useInAppBrowserLinks";
import { LocaleProvider } from "../i18n/LocaleContext";

/** 打开记录落在 window 上，便于 Playwright 断言（与其它交互故事一致）。 */
function recordOpened(url: string) {
  const scope = window as unknown as { __openedLinks?: string[] };
  scope.__openedLinks = scope.__openedLinks ?? [];
  scope.__openedLinks.push(url);
}

function LinkHarness({ enabled }: { enabled: boolean }) {
  const [opened, setOpened] = useState<string[]>([]);
  useInAppBrowserLinks({
    enabled,
    onOpen: (url) => {
      recordOpened(url);
      setOpened((prev) => [...prev, url]);
    },
  });

  useEffect(() => {
    (window as unknown as { __openedLinks?: string[] }).__openedLinks = [];
  }, []);

  return (
    <div
      style={{ padding: 24, display: "grid", gap: 10 }}
      data-enabled={enabled}
    >
      <a data-testid="link-plain" href="https://example.com/news">
        普通网页链接
      </a>
      <a data-testid="link-inner" href="https://example.com/inner">
        <span>嵌套文字链接</span>
      </a>
      <a data-testid="link-fallback" href="/?fallback=1">
        应交给默认行为的链接
      </a>
      <output data-testid="opened">{opened.join("|")}</output>
    </div>
  );
}

const meta = {
  id: "in-app-browser-links",
  title: "Shell/In-App Browser Links",
  component: LinkHarness,
  decorators: [
    (Story) => (
      <LocaleProvider>
        <main
          style={
            {
              boxSizing: "border-box",
              width: "100vw",
              height: "100vh",
              background: "var(--shell-bg)",
              color: "var(--ink)",
            } as CSSProperties
          }
        >
          <Story />
        </main>
      </LocaleProvider>
    ),
  ],
} satisfies Meta<typeof LinkHarness>;

export default meta;
type Story = StoryObj<typeof meta>;

/** 内置浏览器可用：网页链接被接管。 */
export const Default: Story = {
  args: { enabled: true },
};

/** 内置浏览器不可用（如非聊天页 / 非 Tauri）：链接保持默认行为。 */
export const DockUnavailable: Story = {
  args: { enabled: false },
};
