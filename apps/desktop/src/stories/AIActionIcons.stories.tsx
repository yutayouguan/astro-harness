import type { Meta, StoryObj } from "@storybook/react-vite";
import { AIActionIcon } from "../components/icons/AIActionIcon";
import { useTheme } from "../hooks/app/useTheme";
import softMeta from "./SoftMaterial.stories";
import { useState } from "react";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { AiAssistField } from "../components/loop/configs/ConfigField";

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
        <button
          className="loop-icon-btn ai-action-button"
          type="button"
          disabled
          aria-label="不可用的 AI 助手"
        >
          <AIActionIcon size={16} />
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

let completeMockPolish: ((value: string) => void) | null = null;

function PolishIntegration() {
  const [value, setValue] = useState("整理今天的工作");
  const { setMode } = useTheme();
  return (
    <section style={{ maxWidth: 540, padding: 24 }}>
      <h2>真实润色控件 · 模拟请求</h2>
      <p>不调用模型。点击润色后，使用下方按钮完成模拟请求。</p>
      <AiAssistField
        label="任务描述"
        value={value}
        task="任务描述"
        onChange={setValue}
      />
      <button
        type="button"
        onClick={() => {
          completeMockPolish?.("汇总今天已完成的工作、关键进展与待办事项。");
          completeMockPolish = null;
        }}
      >
        完成模拟请求
      </button>
      <button type="button" onClick={() => setValue("")}>
        清空内容
      </button>
      <button type="button" onClick={() => setMode("light")}>
        亮色
      </button>
      <button type="button" onClick={() => setMode("dark")}>
        暗色
      </button>
    </section>
  );
}

export const LivePolish: Story = {
  render: () => <PolishIntegration />,
  beforeEach: () => {
    mockIPC((command) => {
      if (command === "loop_ai_polish")
        return new Promise<string>((resolve) => {
          completeMockPolish = resolve;
        });
      return null;
    });
    return () => {
      completeMockPolish?.("");
      completeMockPolish = null;
      clearMocks();
    };
  },
};
