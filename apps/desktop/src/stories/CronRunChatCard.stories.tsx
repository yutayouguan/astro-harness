import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import {
  CronRunFloatingCard,
  CronTaskDetailDrawer,
  CronRunDetailDrawer,
  type CronJobDto,
  type CronRunDto,
} from "../components/schedule/CronRunDetailDrawer";
import { MorphiconProvider } from "../hooks/app/useMorphicons";
import { DialogProvider, useConfirm } from "../hooks/ui/DialogContext";
import { LocaleProvider } from "../i18n/LocaleContext";
import type { ConversationEntry } from "../types";
import { ChatMarkdown } from "../components/chat/ChatMarkdown";

const run: CronRunDto = {
  id: "run-daily-ai-news",
  job_id: "daily-ai-news",
  title: "每日 AI 新闻推送",
  agent_id: "workspace",
  schedule: "0 9 * * *",
  task: "每天整理 AI 领域重要新闻。",
  fired_at: "2026-08-28T09:30:00+08:00",
  finished_at: "2026-08-28T09:31:18+08:00",
  status: "success",
  summary: "AI 模型、具身智能与芯片领域 10 条精选",
  output: "今日 AI 新闻摘要",
  error: null,
  session_id: "cron-session",
  trigger: "due",
};

const job: CronJobDto = {
  id: "daily-ai-news",
  schedule: "0 8 * * *",
  task: "请搜索并汇总今天 AI 领域最重要的 3-5 条新闻，包括大模型发布、重要论文、行业动态等。",
  title: "每日 AI 新闻推送",
  agent_id: "workspace",
  provider_id: "openai",
  model: "gpt-5.2",
  enabled: true,
  created_at: "2026-08-21T09:00:00+08:00",
  last_run_at: run.fired_at,
  next_run_at: "2026-08-29T08:00:00+08:00",
  show_in_chat: true,
  archived_at: null,
};

const messages: ConversationEntry[] = [
  { id: "user", role: "user", content: run.task },
  {
    id: "assistant",
    role: "assistant",
    content:
      "### 今日 AI 新闻\n\n1. 新一代多模态模型发布。\n2. 具身智能训练集迎来更新。",
  },
];

function CronRunChatPreview() {
  const [taskOpen, setTaskOpen] = useState(false);
  const [runOpen, setRunOpen] = useState(false);
  const confirm = useConfirm();

  return (
    <main
      className="chat-pane has-cron-run"
      style={{
        boxSizing: "border-box",
        height: "100vh",
        background: "var(--shell-bg)",
        color: "var(--ink)",
      }}
    >
      <aside className="chat-cron-run-float">
        <CronRunFloatingCard run={run} onOpen={() => setTaskOpen(true)} />
      </aside>
      <div className="message-list-wrap">
        <div className="message-list">
          <div className="msg-row assistant" style={{ margin: "0 auto" }}>
            <div className="avatar" aria-hidden>
              AI
            </div>
            <div className="msg-stack">
              <article className="bubble assistant">
                <ChatMarkdown content={messages[1].content} />
              </article>
            </div>
          </div>
        </div>
      </div>
      <input
        aria-label="继续追问"
        placeholder="继续追问…"
        style={{
          position: "absolute",
          right: 24,
          bottom: 20,
          left: 24,
          padding: "16px 18px",
          border: "1px solid var(--glass-edge)",
          borderRadius: 18,
          background: "var(--composer-bg)",
          color: "var(--ink-mute)",
          boxShadow: "var(--glass-rim)",
          outline: "none",
        }}
      />
      {taskOpen ? (
        <CronTaskDetailDrawer
          job={job}
          runs={[run]}
          nonModal
          onClose={() => setTaskOpen(false)}
          onEdit={() => {}}
          onToggleEnabled={() => {}}
          onToggleArchived={() => {}}
          onRunNow={() => {}}
          onOpenRun={() => setRunOpen(true)}
        />
      ) : null}
      {runOpen ? (
        <CronRunDetailDrawer
          run={run}
          messages={messages}
          onClose={() => setRunOpen(false)}
          onDelete={() => {
            void confirm({
              title: "删除运行记录",
              message: "确定删除这条定时任务运行记录吗？",
              confirmLabel: "删除记录",
              variant: "danger",
            });
          }}
        />
      ) : null}
    </main>
  );
}

function StorySurface() {
  return (
    <LocaleProvider>
      <MorphiconProvider>
        <DialogProvider>
          <CronRunChatPreview />
        </DialogProvider>
      </MorphiconProvider>
    </LocaleProvider>
  );
}

const meta = {
  title: "Chat/Cron Run Floating Card",
  component: StorySurface,
  parameters: { controls: { disable: true } },
} satisfies Meta<typeof StorySurface>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
