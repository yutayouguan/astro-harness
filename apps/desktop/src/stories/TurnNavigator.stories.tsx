import { useRef } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import TurnNavigator from "../components/chat/TurnNavigator";
import { LocaleProvider } from "../i18n/LocaleContext";
import type { ConversationEntry } from "../types";

const messages: ConversationEntry[] = [
  { id: "q1", role: "user", content: "帮我检查这次聊天界面重构" },
  {
    id: "a1",
    role: "assistant",
    content: "我已检查了消息布局、时间线与窄屏适配。",
  },
  { id: "q2", role: "user", content: "在 review 一下" },
  {
    id: "a2",
    role: "assistant",
    content:
      "[P1] 中文输入法按 Enter 选词会误触发下一题，需要识别 IME 组合输入状态。",
  },
  { id: "q3", role: "user", content: "修复它，并补上测试" },
  { id: "a3", role: "assistant", content: "已修复并通过聚焦测试。" },
  { id: "q4", role: "user", content: "再确认一下窄屏效果" },
  {
    id: "a4",
    role: "assistant",
    content: "窄屏下导航保持在左侧，摘要卡会自动限制宽度。",
  },
];

function TurnNavigatorPreview() {
  const listRef = useRef<HTMLDivElement>(null);
  const bottomRef = useRef<HTMLDivElement>(null);

  return (
    <LocaleProvider>
      <main
        style={{
          minHeight: "100vh",
          boxSizing: "border-box",
          padding: 24,
          background: "var(--shell-bg)",
          color: "var(--ink)",
        }}
      >
        <section
          className="message-list-wrap"
          style={{ width: "min(780px, 100%)", height: 560 }}
        >
          <div className="message-list" ref={listRef}>
            {messages.map((message) => (
              <article
                id={`msg-${message.id}`}
                data-msg-id={message.id}
                className={`msg-row ${message.role}`}
                key={message.id}
              >
                <div className="msg-stack">
                  <div className={`bubble ${message.role}`}>
                    {message.content}
                  </div>
                </div>
              </article>
            ))}
            <div ref={bottomRef} />
          </div>
          <TurnNavigator
            messages={messages}
            listRef={listRef}
            bottomRef={bottomRef}
          />
        </section>
      </main>
    </LocaleProvider>
  );
}

const meta = {
  id: "turn-navigator",
  title: "Conversation/Turn Navigator",
  component: TurnNavigatorPreview,
  parameters: { controls: { disable: true } },
} satisfies Meta<typeof TurnNavigatorPreview>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
