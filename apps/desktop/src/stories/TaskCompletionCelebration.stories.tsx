import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import TaskCompletionCelebration from "../components/chat/TaskCompletionCelebration";

function TaskCompletionCelebrationPreview() {
  const [trigger, setTrigger] = useState(0);

  return (
    <main
      className="chat-pane"
      style={{
        minHeight: "720px",
        padding: "56px",
        boxSizing: "border-box",
        background: "var(--shell-bg)",
        color: "var(--ink)",
      }}
    >
      <TaskCompletionCelebration trigger={trigger} />
      <section
        style={{
          width: "min(720px, 80vw)",
          margin: "auto",
          padding: "28px",
          border: "1px solid var(--line-subtle)",
          borderRadius: "18px",
          background: "var(--surface-glass)",
        }}
      >
        <h2 style={{ marginTop: 0 }}>任务已完成</h2>
        <p>所有步骤已执行并通过检查。庆祝动画只覆盖聊天内容区域。</p>
        <button type="button" onClick={() => setTrigger((value) => value + 1)}>
          再播放一次
        </button>
      </section>
    </main>
  );
}

const meta = {
  id: "chat-task-completion-celebration",
  title: "Chat/Task Completion Celebration",
  component: TaskCompletionCelebrationPreview,
  parameters: {
    layout: "fullscreen",
    controls: { disable: true },
  },
} satisfies Meta<typeof TaskCompletionCelebrationPreview>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
