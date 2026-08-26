import type { Meta, StoryObj } from "@storybook/react-vite";
import { LocaleProvider } from "../i18n/LocaleContext";
import { MorphiconProvider } from "../hooks/app/useMorphicons";
import { ChatMarkdown } from "../components/chat/ChatMarkdown";
import MsgActivity from "../components/chat/MsgActivity";
import { MsgTimeline, MsgTimelineStep } from "../components/chat/MsgTimeline";
import type { ChatActivity } from "../types";

const terminalActivity: ChatActivity = {
  id: "storybook-terminal",
  kind: "tool",
  title: "terminal",
  input: '{"operation":"list","path":"."}',
  output:
    "已读取 **当前目录**。可继续查看 `00-README.md`，或按主题生成摘要。\n\n- 14 篇 Markdown\n- 2 个代码目录",
  status: "done",
  durationSec: 3.6,
};

const answer = `当前工作目录为：

\`\`\`bash
/Users/iswm/Desktop/04-知识库/大模型/八股文
\`\`\`

### 目录内容一览

我已按类型整理了当前目录，可以继续帮你查看文件或总结内容。`;

function ChatAnswerPanel() {
  return (
    <MorphiconProvider>
      <LocaleProvider>
      <main
        data-testid="chat-answer-panel"
        style={{
          minHeight: "100vh",
          boxSizing: "border-box",
          padding: "44px 24px",
          background: "var(--shell-bg)",
          color: "var(--ink)",
        }}
      >
        <div className="msg-row assistant" style={{ margin: "0 auto" }}>
          <div className="avatar" aria-hidden>AI</div>
          <div className="msg-stack">
            <article className="bubble assistant">
              <MsgTimeline>
                <MsgTimelineStep kind="tool">
                  <MsgActivity
                    activity={terminalActivity}
                    defaultOpen
                    showTimestamp={false}
                  />
                </MsgTimelineStep>
                <MsgTimelineStep kind="reply" isLast>
                  <ChatMarkdown content={answer} />
                </MsgTimelineStep>
              </MsgTimeline>
              <div className="msg-token-stats" aria-label="回答统计">
                <span className="msg-token-stats-duration">用时 4.8s</span>
                <span className="msg-token-stats-usage">428 tokens</span>
              </div>
            </article>
          </div>
        </div>
      </main>
      </LocaleProvider>
    </MorphiconProvider>
  );
}

const meta = {
  id: "chat-answer-panel",
  title: "Chat/Answer Panel",
  component: ChatAnswerPanel,
  parameters: {
    controls: { disable: true },
  },
} satisfies Meta<typeof ChatAnswerPanel>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
