import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { Meta, StoryObj } from "@storybook/react-vite";
import CronPanel, { type CronJobDto } from "../components/schedule/CronPanel";
import { MorphiconProvider } from "../hooks/app/useMorphicons";
import { DialogProvider } from "../hooks/ui/DialogContext";
import { LocaleProvider } from "../i18n/LocaleContext";

const activeJobs: CronJobDto[] = [
  {
    id: "daily-ai-news",
    schedule: "0 9 * * *",
    task: "每天为我推送当天最新的 10 条科技新闻，重点关注 AI、具身智能、新能源车和芯片。",
    title: "每日 AI 新闻推送",
    agent_id: "default",
    provider_id: "openai",
    model: "gpt-5.2",
    enabled: true,
    created_at: "2026-08-21T09:00:00+08:00",
    last_run_at: "2026-08-28T09:30:00+08:00",
    next_run_at: "2026-08-29T09:30:00+08:00",
    show_in_chat: true,
    archived_at: null,
    running: false,
  },
  {
    id: "weekly-report",
    schedule: "0 17 * * 5",
    task: "汇总本周项目进展、完成事项、风险和下周计划。",
    title: "每周工作周报",
    agent_id: "default",
    provider_id: "openai",
    model: "gpt-5.2",
    enabled: false,
    created_at: "2026-08-20T17:00:00+08:00",
    last_run_at: "2026-08-22T17:00:00+08:00",
    next_run_at: null,
    show_in_chat: false,
    archived_at: null,
    running: false,
  },
];

const archivedJob: CronJobDto = {
  id: "archived-report",
  schedule: "0 8 * * 1",
  task: "汇总上周归档报告并生成摘要。",
  title: "归档的周报",
  agent_id: "default",
  provider_id: "openai",
  model: "gpt-5.2",
  enabled: true,
  created_at: "2026-08-10T08:00:00+08:00",
  last_run_at: "2026-09-15T08:00:00+08:00",
  next_run_at: null,
  show_in_chat: true,
  archived_at: "2026-09-20T10:00:00+08:00",
  running: false,
};

const jobs: CronJobDto[] = [...activeJobs, archivedJob];

/** 归档 / 删除会改写这份内存列表，让故事里的操作有可见结果。 */
let mockJobs: CronJobDto[] = jobs;

type CronCall = { command: string; payload: unknown };

function recordCronCall(command: string, payload: unknown) {
  const scope = window as unknown as { __cronCalls?: CronCall[] };
  scope.__cronCalls = scope.__cronCalls ?? [];
  scope.__cronCalls.push({ command, payload });
}

const runs = [
  {
    id: "run-3",
    job_id: "daily-ai-news",
    title: "每日 AI 新闻推送",
    agent_id: "default",
    schedule: "0 9 * * *",
    task: jobs[0].task,
    fired_at: "2026-08-28T09:30:00+08:00",
    finished_at: "2026-08-28T09:31:18+08:00",
    status: "success",
    summary: "科技新闻推送：AI、具身智能与芯片领域 10 条精选",
    output: "今日科技新闻摘要",
    error: null,
    session_id: null,
    trigger: "due",
  },
  {
    id: "run-2",
    job_id: "daily-ai-news",
    title: "每日 AI 新闻推送",
    agent_id: "default",
    schedule: "0 9 * * *",
    task: jobs[0].task,
    fired_at: "2026-08-27T09:30:00+08:00",
    finished_at: "2026-08-27T09:30:52+08:00",
    status: "success",
    summary: "用户请求推送科技新闻总结",
    output: "昨日科技新闻摘要",
    error: null,
    session_id: null,
    trigger: "due",
  },
  {
    id: "run-1",
    job_id: "daily-ai-news",
    title: "每日 AI 新闻推送",
    agent_id: "default",
    schedule: "0 9 * * *",
    task: jobs[0].task,
    fired_at: "2026-08-26T09:30:00+08:00",
    finished_at: "2026-08-26T09:31:03+08:00",
    status: "failure",
    summary: "新闻源连接超时",
    output: "",
    error: "Request timed out",
    session_id: null,
    trigger: "due",
  },
];

const meta = {
  title: "Pages/CronPanel",
  component: CronPanel,
  args: {
    active: true,
    providers: [
      { id: "openai", name: "OpenAI", model: "gpt-5.2", kind: "openai" },
    ],
    activeProviderId: "openai",
    tone: "teal",
  },
  beforeEach: () => {
    mockJobs = jobs.map((job) => ({ ...job }));
    (window as unknown as { __cronCalls?: CronCall[] }).__cronCalls = [];
    mockIPC((command, payload) => {
      recordCronCall(command, payload);
      if (command === "list_cron_jobs") return mockJobs;
      if (command === "list_cron_runs") return runs;
      if (command === "list_cron_job_runs") {
        const id = String((payload as { id?: string } | undefined)?.id ?? "");
        return runs.filter((run) => run.job_id === id);
      }
      if (command === "set_cron_job_enabled") return true;
      if (command === "archive_cron_job") {
        const { id, archived } = payload as { id: string; archived: boolean };
        mockJobs = mockJobs.map((job) =>
          job.id === id
            ? {
                ...job,
                archived_at: archived ? "2026-09-27T10:00:00+08:00" : null,
              }
            : job,
        );
        return mockJobs.find((job) => job.id === id) ?? null;
      }
      if (command === "remove_cron_job") {
        const id = (payload as { args?: { id?: string } }).args?.id;
        mockJobs = mockJobs.filter((job) => job.id !== id);
        return true;
      }
      if (command === "run_cron_job_now") return runs[0];
      if (command === "get_cron_run") return runs[0];
      if (command === "get_chat_history") return { items: [] };
      return null;
    });
    return () => clearMocks();
  },
  decorators: [
    (Story) => (
      <LocaleProvider>
        <MorphiconProvider>
          <DialogProvider>
            <main
              style={{
                boxSizing: "border-box",
                width: "100vw",
                height: "100vh",
                padding: 28,
                overflow: "hidden",
                background: "var(--shell-bg)",
                color: "var(--ink)",
              }}
            >
              <Story />
            </main>
          </DialogProvider>
        </MorphiconProvider>
      </LocaleProvider>
    ),
  ],
} satisfies Meta<typeof CronPanel>;

export default meta;
type Story = StoryObj<typeof meta>;

export const CardsWithDetailDrawer: Story = {};

/** 任务运行中：删除与归档都被拦截，只给友好提示。 */
export const RunningTaskGuards: Story = {
  name: "Running Task Guards",
  beforeEach: () => {
    mockJobs = activeJobs.map((job) => ({
      ...job,
      running: job.id === "daily-ai-news",
    }));
  },
};
