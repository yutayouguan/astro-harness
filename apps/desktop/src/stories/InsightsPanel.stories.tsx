import type { Meta, StoryObj } from "@storybook/react-vite";
import InsightsPanel, {
  type TraceInsights,
  type UsageInsights,
} from "../components/settings/InsightsPanel";
import { ActiveAgentProvider } from "../hooks/app/useActiveAgent";
import { LocaleProvider } from "../i18n/LocaleContext";

const now = new Date();
const year = now.getUTCFullYear();
const month = now.getUTCMonth();
const today = now.getUTCDate();
const daysInMonth = new Date(Date.UTC(year, month + 1, 0)).getUTCDate();
const bucket = (day: number) =>
  `${year}-${String(month + 1).padStart(2, "0")}-${String(day).padStart(2, "0")}`;
const dailyUsage = [
  { calls: 76, tokens: 45_100, cost_usd: 0.64 },
  { calls: 49, tokens: 13_800, cost_usd: 0.19 },
  { calls: 83, tokens: 47_800, cost_usd: 0.68 },
  { calls: 112, tokens: 272_100, cost_usd: 3.31 },
];

const sampleData: UsageInsights = {
  kpis: {
    calls: 675,
    tokens: 378_800,
    cost_usd: 4.82,
    active_agents: 3,
    llm_calls: 320,
    input_tokens: 232_400,
    output_tokens: 102_900,
    cache_tokens: 39_000,
    reasoning_tokens: 4_500,
  },
  series: Array.from({ length: daysInMonth }, (_, index) => {
    const day = index + 1;
    const usage = day <= today ? dailyUsage[index] : undefined;
    return {
      bucket: bucket(day),
      calls: usage?.calls ?? 0,
      tokens: usage?.tokens ?? 0,
      cost_usd: usage?.cost_usd ?? 0,
    };
  }),
  rankings: {
    by_kind: [
      { kind: "tool", name: "browser", calls: 128, tokens: 0, cost_usd: 0 },
      { kind: "tool", name: "terminal", calls: 86, tokens: 0, cost_usd: 0 },
      {
        kind: "skill",
        name: "code-review",
        calls: 34,
        tokens: 0,
        cost_usd: 0,
      },
      { kind: "mcp", name: "lark-doc", calls: 22, tokens: 0, cost_usd: 0 },
      {
        kind: "cron",
        name: "daily-brief",
        calls: 11,
        tokens: 0,
        cost_usd: 0,
      },
    ],
    by_agent: [
      {
        kind: "agent",
        name: "default",
        calls: 210,
        tokens: 252_000,
        cost_usd: 3.4,
      },
      {
        kind: "agent",
        name: "research",
        calls: 110,
        tokens: 126_800,
        cost_usd: 1.42,
      },
    ],
    by_model: [
      {
        kind: "llm",
        name: "openai/gpt-5.6",
        calls: 180,
        tokens: 232_400,
        cost_usd: 3.7,
      },
      {
        kind: "llm",
        name: "deepseek/deepseek-chat",
        calls: 96,
        tokens: 144_500,
        cost_usd: 1.12,
      },
      {
        kind: "llm",
        name: "custom-model",
        calls: 38,
        tokens: 1_564,
        cost_usd: 0,
      },
      { kind: "llm", name: "", calls: 6, tokens: 336, cost_usd: 0 },
    ],
  },
  unpriced_llm_events: 44,
};

const sampleTraces: TraceInsights = {
  kpis: { traces: 2, events: 4, llm: 2, tools: 1, skills: 1 },
  traces: [
    {
      session_id: "session-research-01",
      agent_id: "research",
      title: "整理本周研究资料",
      started_at: "2026-09-07T09:12:00Z",
      ended_at: "2026-09-07T09:18:32Z",
      event_count: 2,
      tokens: 11_940,
      cost_usd: 0.28,
      kinds: ["user", "llm", "tool", "skill"],
      events: [
        {
          id: "research-llm",
          ts: "2026-09-07T09:12:02Z",
          kind: "llm",
          name: "openai/gpt-5.6",
          agent_id: "research",
          input_tokens: 9_840,
          output_tokens: 2_100,
          total_tokens: 11_940,
          cost_usd: 0.28,
          duration_ms: 5_420,
          turn_id: "turn-research",
          status: "done",
        },
        {
          id: "research-skill",
          ts: "2026-09-07T09:13:14Z",
          kind: "skill",
          name: "research-digest",
          agent_id: "research",
          input_tokens: 0,
          output_tokens: 0,
          total_tokens: 0,
          cost_usd: 0,
          duration_ms: 1_260,
          turn_id: "turn-research",
          status: "done",
        },
      ],
    },
    {
      session_id: "session-code-02",
      agent_id: "default",
      title: "修复桌面端设置回归",
      started_at: "2026-09-07T08:40:00Z",
      ended_at: "2026-09-07T08:47:18Z",
      event_count: 2,
      tokens: 9_740,
      cost_usd: 0.24,
      kinds: ["user", "llm", "tool"],
      events: [
        {
          id: "code-llm",
          ts: "2026-09-07T08:40:03Z",
          kind: "llm",
          name: "openai/gpt-5.6",
          agent_id: "default",
          input_tokens: 7_800,
          output_tokens: 1_940,
          total_tokens: 9_740,
          cost_usd: 0.24,
          duration_ms: 4_820,
          turn_id: "turn-code",
          status: "done",
        },
        {
          id: "code-tool",
          ts: "2026-09-07T08:41:26Z",
          kind: "tool",
          name: "exec_command",
          agent_id: "default",
          input_tokens: 0,
          output_tokens: 0,
          total_tokens: 0,
          cost_usd: 0,
          duration_ms: 860,
          turn_id: "turn-code",
          status: "done",
        },
      ],
    },
  ],
};

function InsightsPreview({
  initialView = "overview",
}: {
  initialView?: "overview" | "models" | "tools" | "tracing";
}) {
  return (
    <LocaleProvider>
      <ActiveAgentProvider>
        <main
          style={{
            width: "100%",
            minHeight: "100vh",
            padding: 28,
            background: "var(--shell-bg)",
          }}
        >
          <InsightsPanel
            active
            initialData={sampleData}
            initialTraces={sampleTraces}
            initialView={initialView}
          />
        </main>
      </ActiveAgentProvider>
    </LocaleProvider>
  );
}

const meta = {
  title: "Settings/InsightsPanel",
  component: InsightsPreview,
  parameters: {
    viewport: { defaultViewport: "responsive" },
  },
} satisfies Meta<typeof InsightsPreview>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Overview: Story = {};
export const ModelUsage: Story = {
  render: () => <InsightsPreview initialView="models" />,
};
export const ToolsAndSkills: Story = {
  render: () => <InsightsPreview initialView="tools" />,
};
export const Tracing: Story = {
  render: () => <InsightsPreview initialView="tracing" />,
};
