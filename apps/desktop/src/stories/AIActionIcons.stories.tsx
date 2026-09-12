import type { Meta, StoryObj } from "@storybook/react-vite";
import { AIActionIcon } from "../components/icons/AIActionIcon";
import { useTheme } from "../hooks/app/useTheme";
import softMeta from "./SoftMaterial.stories";

function AIActionIcons() {
  const { setMode } = useTheme();
  return (
    <section style={{ display: "grid", gap: 28, padding: 24 }}>
      <header>
        <h2>AI 动作图标</h2>
        <p>生成与智能搜索 · 青蓝 / 紫 / 粉渐变</p>
      </header>
      <div style={{ display: "flex", gap: 20 }}>
        <AIActionIcon size={84} framed />
        <AIActionIcon size={84} framed variant="search" />
      </div>
      <div style={{ display: "flex", gap: 20, alignItems: "center" }}>
        {[13, 16, 20, 24, 32].map((size) => (
          <AIActionIcon key={size} size={size} />
        ))}
        <button
          className="loop-icon-btn ai-action-button"
          type="button"
          aria-label="AI 助手"
        >
          <AIActionIcon size={16} />
        </button>
        <button
          className="loop-icon-btn ai-action-button is-active"
          type="button"
          aria-label="AI 搜索"
          aria-pressed="true"
        >
          <AIActionIcon size={16} variant="search" />
        </button>
      </div>
      <div
        style={{
          display: "flex",
          gap: 20,
          alignItems: "center",
          flexWrap: "wrap",
        }}
      >
        <button type="button" className="wallpaper-primary-button">
          <AIActionIcon size={15} />
          生成壁纸
        </button>
        <button type="button" className="desktop-pet-generate">
          <AIActionIcon size={17} />
          生成桌宠
        </button>
        <button type="button" className="loop-config-ai-btn">
          <AIActionIcon size={13} />
          AI 润色
        </button>
        <button type="button" className="wallpaper-primary-button" disabled>
          <AIActionIcon size={15} />
          不可用
        </button>
      </div>
      <div style={{ display: "flex", gap: 12 }}>
        <button type="button" onClick={() => setMode("light")}>
          亮色
        </button>
        <button type="button" onClick={() => setMode("dark")}>
          暗色
        </button>
      </div>
    </section>
  );
}
const meta = {
  title: "Design/AI Action Icons",
  component: AIActionIcons,
  decorators: softMeta.decorators,
  beforeEach: softMeta.beforeEach,
} satisfies Meta<typeof AIActionIcons>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Gallery: Story = {};
