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
import ChatView, {
  MessageActions,
  MessageTokenStats,
} from "../components/chat/ChatView";
import type { ChatActivity, ConversationEntry } from "../types";

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
    input:
      '{"operation":"read","path":"apps/desktop/src/components/chat/ChatView.tsx"}',
    output: "读取 2146 行",
    status: "done",
    durationSec: 0.4,
    batchId: "storybook-analysis",
    executionMode: "parallel",
  },
  {
    id: "storybook-search",
    kind: "tool",
    title: "exec_command",
    input: '{"operation":"search","query":"msg-activity"}',
    output: "找到 18 处匹配",
    status: "done",
    durationSec: 0.2,
    batchId: "storybook-analysis",
    executionMode: "parallel",
  },
  {
    id: "storybook-run",
    kind: "tool",
    title: "exec_command",
    input: '{"cmd":"npm run build"}',
    output: "Build completed",
    status: "done",
    durationSec: 3.6,
    batchId: "storybook-analysis",
    executionMode: "parallel",
  },
];

const answer = `当前工作目录为：

\`\`\`bash
/Users/iswm/Desktop/04-知识库/大模型/八股文
\`\`\`

### 目录内容一览

我已按类型整理了当前目录，可以继续帮你查看文件或总结内容。`;

const codeBlockShowcase = `### 实测结果

| URL | 抓到的标题 |
| --- | --- |
| example.com | Example Domain |
| python.org | Welcome to Python.org |
| github.com | GitHub · Change is constant... |

### 用法

\`\`\`bash
python3 fetch_title.py https://www.python.org
\`\`\`

### 实现要点

- 用 \`urllib.request\` 请求，\`html.parser.HTMLParser\` 提取 \`<title>\` 内容
- 带 \`User-Agent\` 头，避免部分站点拒绝默认 UA

### 多行代码

\`\`\`typescript
async function fetchTitle(url: string) {
  const response = await fetch(url);
  return response.text();
}
\`\`\``;

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
            <div className="avatar" aria-hidden>
              AI
            </div>
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
                  <MsgTimelineStep kind="reply">
                    <ChatMarkdown content="已经完成目录扫描，接下来检查构建状态。" />
                  </MsgTimelineStep>
                  <MsgTimelineStep kind="reasoning">
                    <MsgReasoning
                      reasoning="根据目录结果选择最小验证范围。"
                      active={false}
                      durationSec={12}
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
                <div className="assistant-message-footer">
                  <MessageActions
                    messageId="storybook-assistant"
                    content={answer}
                    role="assistant"
                    onRegenerate={() => {}}
                    onBranch={() => {}}
                    onOpenMenu={() => {}}
                  />
                  <MessageTokenStats
                    generationDurationSec={258}
                    tokensPerSec={41.4}
                    usage={{
                      promptTokens: 188_650,
                      uncachedInputTokens: 169_049,
                      completionTokens: 10_683,
                      totalTokens: 199_333,
                      cacheReadTokens: 19_601,
                      cacheWriteTokens: 0,
                      reasoningTokens: 2_990,
                      requestCount: 1,
                      cacheReadReported: true,
                      cacheWriteReported: false,
                      reasoningReported: true,
                    }}
                  />
                </div>
              </article>
            </div>
          </div>
        </main>
      </LocaleProvider>
    </MorphiconProvider>
  );
}

function CodeBlockShowcase() {
  return (
    <MorphiconProvider>
      <LocaleProvider>
        <main
          style={{
            minHeight: "100vh",
            boxSizing: "border-box",
            padding: "44px 24px",
            background: "var(--shell-bg)",
            color: "var(--ink)",
          }}
        >
          <div className="msg-row assistant" style={{ margin: "0 auto" }}>
            <div className="msg-stack">
              <article className="bubble assistant">
                <ChatMarkdown content={codeBlockShowcase} />
              </article>
            </div>
          </div>
        </main>
      </LocaleProvider>
    </MorphiconProvider>
  );
}

function GroupedAnswerLayoutPreview() {
  const activities = groupedActivities.slice(0, 2);
  const messages: ConversationEntry[] = [
    {
      id: "grouped-answer",
      role: "assistant",
      reasoning: "先梳理目标，再核对工具输出。",
      reasoningDurationSec: 2.4,
      content:
        "已完成目录检查。\n\n### 结果\n\n所有正文片段都已合并为一个 Markdown 回答。",
      activities,
      segments: [
        { type: "reasoning", id: "reasoning-1", text: "先梳理目标，", at: 1 },
        { type: "activity", id: activities[0]!.id, at: 2 },
        { type: "text", id: "text-1", text: "已完成目录检查。", at: 3 },
        {
          type: "reasoning",
          id: "reasoning-2",
          text: "再核对工具输出。",
          at: 4,
        },
        { type: "activity", id: activities[1]!.id, at: 5 },
        {
          type: "text",
          id: "text-2",
          text: "\n\n### 结果\n\n所有正文片段都已合并为一个 Markdown 回答。",
          at: 6,
        },
      ],
    },
  ];

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
                answerLayout: "grouped",
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
            />
          </main>
        </DialogProvider>
      </LocaleProvider>
    </MorphiconProvider>
  );
}

function InlineUserEditPreview() {
  const [messages, setMessages] = useState<ConversationEntry[]>([
    { id: "u-1", role: "user", content: "先给我解释一下这个模块。" },
    {
      id: "a-1",
      role: "assistant",
      content: "这是上一轮回答，历史问题不可编辑。",
    },
    {
      id: "u-2",
      role: "user",
      content: "把结论整理得更简洁，并补充一个示例。",
    },
    {
      id: "a-2",
      role: "assistant",
      content: "这是当前回答；最后一个问题可以原位修改。",
    },
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
                answerLayout: "timeline",
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
                    message.id === messageId
                      ? { ...message, content }
                      : message,
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
                answerLayout: "timeline",
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

export const GroupedLayout: Story = {
  render: () => <GroupedAnswerLayoutPreview />,
};

export const CodeBlocks: Story = {
  render: () => <CodeBlockShowcase />,
};

export const InlineUserEdit: Story = {
  render: () => <InlineUserEditPreview />,
};

export const WelcomeLogoInteraction: Story = {
  render: () => <WelcomeLogoInteractionPreview />,
};
