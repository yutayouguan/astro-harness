import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { Meta, StoryObj } from "@storybook/react-vite";
import LoopPanel from "../components/loop/LoopPanel";
import type { LoopDto } from "../components/loop/loopTypes";
import { DialogProvider } from "../hooks/ui/DialogContext";
import { LocaleProvider } from "../i18n/LocaleContext";

const workflows: LoopDto[] = [
  {
    id: "daily-summary",
    name: "每日内容摘要",
    description: "收集工作区更新，生成结构化摘要并保存。",
    enabled: true,
    ai_callable: true,
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
      return null;
    });
    return () => clearMocks();
  },
  decorators: [
    (Story) => (
      <LocaleProvider>
        <DialogProvider>
          <main
            style={{
              boxSizing: "border-box",
              width: "100vw",
              height: "100vh",
              overflow: "hidden",
              background: "var(--shell-bg)",
              color: "var(--ink)",
            }}
          >
            <Story />
          </main>
        </DialogProvider>
      </LocaleProvider>
    ),
  ],
} satisfies Meta<typeof LoopPanel>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
