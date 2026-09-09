import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { Meta, StoryObj } from "@storybook/react-vite";
import LoopPanel from "../components/loop/LoopPanel";
import type { LoopDto } from "../components/loop/loopTypes";
import { DialogProvider } from "../hooks/ui/DialogContext";
import { MorphiconProvider } from "../hooks/app/useMorphicons";
import { LocaleProvider } from "../i18n/LocaleContext";

const workflows: LoopDto[] = [
  {
    id: "daily-summary",
    name: "每日内容摘要",
    description: "收集工作区更新，生成结构化摘要并保存。",
    enabled: true,
    agent_tool: {
      exposure: "deferred",
      name: "daily_summary",
      input_schema: {
        type: "object",
        properties: {
          topic: { type: "string", description: "要摘要的主题" },
        },
        additionalProperties: false,
      },
      output_description: "结构化的每日摘要",
      examples: [{ topic: "Astro 开发进展" }],
      confirmation: "auto",
    },
    nodes: [
      {
        id: "manual",
        node_type: "manual_trigger",
        label: "手动触发",
        position: { x: 80, y: 120 },
        config: {},
        disabled: false,
      },
      {
        id: "summary",
        node_type: "summarization",
        label: "生成摘要",
        position: { x: 340, y: 120 },
        config: {},
        disabled: false,
      },
    ],
    edges: [
      {
        id: "manual-summary",
        source: "manual",
        source_handle: null,
        target: "summary",
        target_handle: null,
      },
    ],
    variables: {},
    created_at: "2026-08-28T09:00:00+08:00",
    updated_at: "2026-08-28T12:00:00+08:00",
  },
];

const meta = {
  title: "Pages/LoopPanel",
  component: LoopPanel,
  args: {
    active: true,
    providers: [
      { id: "openai", name: "OpenAI", model: "gpt-5.2", kind: "openai" },
    ],
  },
  beforeEach: () => {
    mockIPC((command) => {
      if (command === "list_loops") return workflows;
      if (command === "list_loop_runs") return [];
      if (command === "get_loop") return workflows[0];
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
                display: "flex",
                flexDirection: "column",
                overflow: "hidden",
                padding: "18px 22px 22px",
                background: "var(--shell-bg)",
                color: "var(--ink)",
              }}
            >
              <div className="native-drag-region" aria-hidden />
              <div className="feature-content-inline">
                <Story />
              </div>
            </main>
          </DialogProvider>
        </MorphiconProvider>
      </LocaleProvider>
    ),
  ],
} satisfies Meta<typeof LoopPanel>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const Empty: Story = {
  beforeEach: () => {
    mockIPC((command) => {
      if (command === "list_loops" || command === "list_loop_runs") return [];
      return null;
    });
    return () => clearMocks();
  },
};
