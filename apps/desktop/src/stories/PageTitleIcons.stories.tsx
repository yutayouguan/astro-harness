import type { Meta, StoryObj } from "@storybook/react-vite";
import { Brain, Settings2, Wrench } from "lucide-react";
import { IconTools } from "../components/icons/NavIcons";

const samples = [
  { label: "工具", tone: "pink", Icon: Wrench },
  { label: "记忆", tone: "purple", Icon: Brain },
  { label: "设置", tone: "twilight", Icon: Settings2 },
  { label: "自定义工具图标", tone: "indigo", Icon: IconTools },
] as const;

function PageTitleIcons() {
  return (
    <main
      style={{
        minHeight: "100vh",
        padding: 32,
        background: "var(--shell-bg)",
      }}
    >
      <div style={{ display: "grid", gap: 18, maxWidth: 520 }}>
        {samples.map(({ label, tone, Icon }) => (
          <div className="content-header" key={label}>
            <div className="content-heading">
              <div className="page-title-block">
                <div className="page-title-icon" data-tone={tone} aria-hidden>
                  <Icon width={22} height={22} strokeWidth={1.6} />
                </div>
                <div className="page-title-text">
                  <h1 className="content-title" data-tone={tone}>
                    <span className="content-title-main">{label}</span>
                  </h1>
                </div>
              </div>
            </div>
          </div>
        ))}
      </div>
    </main>
  );
}

const meta = {
  title: "Shell/PageTitleIcons",
  component: PageTitleIcons,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof PageTitleIcons>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Outline: Story = {};
