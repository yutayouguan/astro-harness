import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { BarChart3, Grid2x2 } from "lucide-react";
import ModelRankingsPanel from "../components/settings/ModelRankingsPanel";
import { LocaleProvider } from "../i18n/LocaleContext";

const categories = [
  { key: "code", label: "Code", spendShare: 0.304 },
  { key: "data", label: "Data", spendShare: 0.088 },
  { key: "agent", label: "Agent", spendShare: 0.285 },
  { key: "general", label: "General", spendShare: 0.323 },
];

const models = [
  { model: "anthropic/claude-opus-5", share: 0.133, deltaPp: 10.3 },
  { model: "moonshot/kimi-k3", share: 0.089, deltaPp: 4.7 },
  { model: "openai/gpt-5.6-sol", share: 0.082, deltaPp: 3.8 },
  { model: "z-ai/glm-5.2", share: 0.059, deltaPp: -2.5 },
  { model: "anthropic/claude-4.8-opus", share: 0.059, deltaPp: -11.5 },
  { model: "deepseek/deepseek-v4-pro", share: 0.049, deltaPp: 0.8 },
  { model: "google/gemini-3.6-flash", share: 0.039, deltaPp: 3.4 },
];

const taskData = {
  windowDays: 30,
  macroCategories: categories,
  tasks: [
    {
      tag: "agent:workflow_execution",
      macroCategory: "agent",
      spendShareOfTotal: 0.187,
      models,
    },
    {
      tag: "code:general_impl",
      macroCategory: "code",
      spendShareOfTotal: 0.107,
      models: models.slice(1),
    },
    {
      tag: "classification_tagging",
      macroCategory: "general",
      spendShareOfTotal: 0.1,
      models: models.slice(2),
    },
    {
      tag: "agent:multi_step_planning",
      macroCategory: "agent",
      spendShareOfTotal: 0.065,
      models: models.slice(3),
    },
    {
      tag: "data:extraction",
      macroCategory: "data",
      spendShareOfTotal: 0.06,
      models: models.slice(0, 5),
    },
    {
      tag: "code:debugging",
      macroCategory: "code",
      spendShareOfTotal: 0.056,
      models: models.slice(1),
    },
    {
      tag: "content_writing",
      macroCategory: "general",
      spendShareOfTotal: 0.043,
      models: models.slice(2),
    },
    {
      tag: "qa_knowledge",
      macroCategory: "general",
      spendShareOfTotal: 0.04,
      models: models.slice(0, 4),
    },
    {
      tag: "roleplay_fiction",
      macroCategory: "general",
      spendShareOfTotal: 0.038,
      models: models.slice(3),
    },
    {
      tag: "code:file_read_write",
      macroCategory: "code",
      spendShareOfTotal: 0.038,
      models: models.slice(1),
    },
    {
      tag: "code:shell_execution",
      macroCategory: "code",
      spendShareOfTotal: 0.034,
      models: models.slice(2),
    },
    {
      tag: "data:transformation",
      macroCategory: "data",
      spendShareOfTotal: 0.028,
      models: models.slice(0, 5),
    },
    {
      tag: "code:frontend_ui",
      macroCategory: "code",
      spendShareOfTotal: 0.027,
      models: models.slice(1),
    },
    {
      tag: "code:review_security",
      macroCategory: "code",
      spendShareOfTotal: 0.024,
      models: models.slice(2),
    },
    {
      tag: "conversational_reply",
      macroCategory: "general",
      spendShareOfTotal: 0.023,
      models: models.slice(0, 4),
    },
  ],
};

const meta = {
  id: "model-rankings-panel",
  title: "Settings/ModelRankingsPanel",
  component: ModelRankingsPanel,
  args: { active: true },
  beforeEach: () => {
    mockIPC((command) => {
      if (command !== "get_openrouter_rankings") return null;
      return {
        dataset: "tasks",
        modality: null,
        dataSource: "frontend",
        freshness: "fresh",
        cacheHit: false,
        usedFallback: false,
        fetchedAt: "2026-09-04T09:02:00Z",
        asOf: "2026-09-04T09:02:00Z",
        payload: {
          data: {
            spend: taskData,
            tokens: taskData,
          },
        },
      };
    });
    return () => clearMocks();
  },
  decorators: [
    (Story) => (
      <LocaleProvider>
        <main
          style={{
            boxSizing: "border-box",
            width: "100vw",
            height: "100vh",
            padding: "18px 28px",
            overflow: "hidden",
            background: "var(--shell-bg)",
            color: "var(--ink)",
          }}
        >
          <Story />
        </main>
      </LocaleProvider>
    ),
  ],
} satisfies Meta<typeof ModelRankingsPanel>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Tasks: Story = {};

export const EmbeddedInModelMarket: Story = {
  render: (args) => (
    <div className="model-market">
      <div className="model-market-surface-tabs" role="tablist">
        <button type="button" role="tab" aria-selected="false">
          <Grid2x2 size={15} />
          模型目录
        </button>
        <button
          type="button"
          role="tab"
          aria-selected="true"
          className="active"
        >
          <BarChart3 size={15} />
          模型情报
        </button>
      </div>
      <ModelRankingsPanel {...args} />
    </div>
  ),
};
