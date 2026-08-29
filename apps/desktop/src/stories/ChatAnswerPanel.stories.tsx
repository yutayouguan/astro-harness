import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { LocaleProvider } from "../i18n/LocaleContext";
import { MorphiconProvider } from "../hooks/app/useMorphicons";
import { DialogProvider } from "../hooks/ui/DialogContext";
import { ChatMarkdown } from "../components/chat/ChatMarkdown";
import MsgActivity from "../components/chat/MsgActivity";
import MsgActivityGroup from "../components/chat/MsgActivityGroup";
import MsgReasoning from "../components/chat/MsgReasoning";
import { MsgTimeline, MsgTimelineStep } from "../components/chat/MsgTimeline";
import ChatView, { MessageActions } from "../components/chat/ChatView";
import type { ChatActivity, ChatMessage } from "../types";

const terminalActivity: ChatActivity = {
  id: "storybook-terminal",
  kind: "tool",
  title: "exec_command",
  input: '{"operation":"list","path":"."}',
  output:
    "已读取 **当前目录**。可继续查看 `00-README.md`，或按主题生成摘要。\n\n- 14 篇 Markdown\n- 2 个代码目录",
  status: "running",
};

const groupedActivities: ChatActivity[] = [
  {
    id: "storybook-read",
    kind: "tool",
    title: "exec_command",
    input: '{"operation":"read","path":"apps/desktop/src/components/chat/ChatView.tsx"}',
    output: "读取 2146 行",
    status: "done",
    durationSec: 0.4,
  },
  {
    id: "storybook-search",
    kind: "tool",
    title: "exec_command",
    input: '{"operation":"search","query":"msg-activity"}',
    output: "找到 18 处匹配",
    status: "done",
    durationSec: 0.2,
  },
  {
    id: "storybook-run",
    kind: "tool",
    title: "exec_command",
    input: '{"cmd":"npm run build"}',
    output: "Build completed",
    status: "done",
    durationSec: 3.6,
  },
];

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
                <MsgTimelineStep kind="reasoning">
                  <MsgReasoning
                    reasoning="先梳理用户目标，再检查工具执行结果与当前任务进度。"
                    active={false}
                    durationSec={65}
                  />
                </MsgTimelineStep>
                <MsgTimelineStep kind="tool">
                  <MsgActivityGroup
                    activities={groupedActivities}
                    defaultOpen
                    showTimestamp={false}
                  />
                </MsgTimelineStep>
                <MsgTimelineStep kind="tool">
                  <MsgActivity
                    activity={terminalActivity}
                    defaultOpen={false}
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
            <MessageActions
              messageId="storybook-assistant"
              content={answer}
              role="assistant"
              onRegenerate={() => {}}
              onBranch={() => {}}
            />
          </div>
        </div>
      </main>
      </LocaleProvider>
    </MorphiconProvider>
  );
}

function InlineUserEditPreview() {
  const [messages, setMessages] = useState<ChatMessage[]>([
    { id: "u-1", role: "user", content: "先给我解释一下这个模块。" },
    { id: "a-1", role: "assistant", content: "这是上一轮回答，历史问题不可编辑。" },
    { id: "u-2", role: "user", content: "把结论整理得更简洁，并补充一个示例。" },
    { id: "a-2", role: "assistant", content: "这是当前回答；最后一个问题可以原位修改。" },
  ]);

  return (
    <MorphiconProvider>
      <LocaleProvider>
        <DialogProvider>
          <main style={{ height: "760px", background: "var(--shell-bg)" }}>
            <ChatView
              messages={messages}
              input=""
              attachments={[]}
              streaming={false}
              displayPrefs={{
                verbosity: "normal",
                showTools: true,
                showSkills: true,
                showMcp: false,
                showHooks: true,
                showMemory: true,
                showStatus: true,
                showTimestamps: false,
              }}
              emptyMode={null}
              onInputChange={() => {}}
              onAttachmentsChange={() => {}}
              onSend={() => {}}
              onNewChat={() => {}}
              onPickWelcomePrompt={() => {}}
              thinkingPrefs={{ level: "off" }}
              onToggleThinking={() => {}}
              onThinkingLevelChange={() => {}}
              chatMode="agent"
              onChatModeChange={() => {}}
              onOpenContext={() => {}}
              onRegenerateMessage={() => {}}
              onEditUserMessage={async (messageId, content) => {
                setMessages((current) =>
                  current.map((message) =>
                    message.id === messageId ? { ...message, content } : message,
                  ),
                );
                return true;
              }}
              onBranchMessage={() => {}}
            />
          </main>
        </DialogProvider>
      </LocaleProvider>
    </MorphiconProvider>
  );
}

function WelcomeLogoInteractionPreview() {
  const [input, setInput] = useState("");

  return (
    <MorphiconProvider>
      <LocaleProvider>
        <DialogProvider>
          <main style={{ height: "760px", background: "var(--shell-bg)" }}>
            <ChatView
              messages={[]}
              input={input}
              attachments={[]}
              streaming={false}
              displayPrefs={{
                verbosity: "normal",
                showTools: true,
                showSkills: true,
                showMcp: false,
                showHooks: true,
                showMemory: true,
                showStatus: true,
                showTimestamps: false,
              }}
              emptyMode="chat"
              onInputChange={setInput}
              onAttachmentsChange={() => {}}
              onSend={() => {}}
              onNewChat={() => {}}
              onPickWelcomePrompt={setInput}
              thinkingPrefs={{ level: "off" }}
              onToggleThinking={() => {}}
              onThinkingLevelChange={() => {}}
              chatMode="agent"
              onChatModeChange={() => {}}
              onOpenContext={() => {}}
            />
          </main>
        </DialogProvider>
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

export const InlineUserEdit: Story = {
  render: () => <InlineUserEditPreview />,
};

export const WelcomeLogoInteraction: Story = {
  render: () => <WelcomeLogoInteractionPreview />,
};
