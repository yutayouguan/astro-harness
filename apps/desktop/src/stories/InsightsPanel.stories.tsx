import type { Meta, StoryObj } from "@storybook/react-vite";
import InsightsPanel, {
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
    by_kind: [],
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

function InsightsPreview() {
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
          <InsightsPanel active initialData={sampleData} />
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
