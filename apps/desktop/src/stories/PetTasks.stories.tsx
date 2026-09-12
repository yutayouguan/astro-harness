import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { Meta, StoryObj } from "@storybook/react-vite";
import PetTaskSurface from "../components/desktop-pet/PetTaskSurface";
import "../styles/features/pet-tasks.css";
import { LocaleProvider } from "../i18n/LocaleContext";
import { receiveInteractionState } from "../hooks/chat/usePendingInteractions";
import type {
  InteractionState,
  PendingInteraction,
} from "../lib/chat/pendingInteractions";

const approval: PendingInteraction = {
  key: "approval-key",
  sessionId: "s1",
  turnId: "t1",
  requestId: "r1",
  toolCallId: "call1",
  kind: "approval",
  message: "读取项目文件",
  operations: [
    {
      updateComponents: {
        components: [
          {
            variant: "approval",
            body: "工作目录：/project\n仅读取文件\n```sh\ncat README.md\n```",
            allowAlways: true,
          },
        ],
      },
    },
  ],
  responseSchema: {
    type: "object",
    properties: { approved: { type: "boolean" } },
  },
  actions: [
    {
      id: "approve",
      label: "仅本次允许",
      payload: { approved: true },
      persistent: false,
    },
    {
      id: "approve_always",
      label: "永久允许此操作",
      payload: { approved: true, always: true },
      persistent: true,
    },
    {
      id: "deny",
      label: "拒绝",
      payload: { approved: false },
      persistent: false,
    },
  ],
  expiresAt: "",
  serverName: null,
  generation: null,
};
const question: PendingInteraction = {
  ...approval,
  key: "question-key",
  requestId: "r2",
  toolCallId: "call2",
  kind: "question",
  message: "请选择部署环境",
  operations: [],
  actions: [],
  responseSchema: {
    type: "object",
    properties: {
      environment: {
        type: "string",
        title: "部署环境",
        enum: ["测试", "生产"],
      },
      note: { type: "string", title: "备注" },
    },
    required: ["environment"],
  },
};
const meta = {
  title: "Desktop/PetTasks",
  component: PetTaskSurface,
  args: { preview: true },
  beforeEach: (context) => {
    let shouldFail = Boolean(context.parameters.failFirst);
    const activeQuestion = context.parameters.wizard
      ? {
          ...question,
          message: "请完成两步确认",
          operations: [
            {
              updateComponents: {
                components: [
                  {
                    component: "ClarifyWizard",
                    steps: [
                      {
                        id: "target",
                        question: "原始问题一：请选择目标",
                        options: ["本地", "测试"],
                      },
                      {
                        id: "note",
                        question: "原始问题二：请补充说明",
                        options: [],
                      },
                    ],
                  },
                ],
              },
            },
          ],
          responseSchema: {
            type: "object",
            required: ["answers", "value"],
            properties: {
              answers: { type: "object" },
              value: { type: "string" },
            },
          },
        }
      : question;
    let state: InteractionState = {
      expanded: !context.parameters.collapsed,
      uiRevision: 1,
      connected: true,
      selected: context.parameters.list
        ? null
        : context.parameters.failFirst
          ? question.key
          : approval.key,
      snapshot: {
        epoch: crypto.randomUUID(),
        revision: 1,
        tasks: [
          {
            sessionId: "s1",
            turnId: "t1",
            title: "整理项目",
            project: "/project",
            parentSessionId: null,
            status: "waiting",
          },
        ],
        requests: [approval, activeQuestion],
      },
    };
    receiveInteractionState(state);
    mockIPC(async (command, payload) => {
      if (command === "plugin:event|listen") return 1;
      if (command === "plugin:event|unlisten") return null;
      if (command === "get_pending_interactions") return structuredClone(state);
      if (command === "open_pet_tasks") {
        state = {
          ...state,
          expanded: true,
          selected: (payload as { requestKey: string | null }).requestKey,
        };
        receiveInteractionState(state);
        return null;
      }
      if (command === "dismiss_pet_tasks") {
        state = { ...state, expanded: false };
        receiveInteractionState(state);
        return null;
      }
      if (command === "focus_pet_tasks" || command === "open_pet_task_session")
        return null;
      if (command === "respond_pending_interaction") {
        if (shouldFail) {
          shouldFail = false;
          throw new Error("模拟网络失败，输入仍保留");
        }
        const request = (
          payload as {
            request: {
              key: string;
              action: string;
              confirmedPersistent: boolean;
              payload: unknown;
            };
          }
        ).request;
        if (request.action === "approve_always" && !request.confirmedPersistent)
          throw new Error("未确认长期授权");
        await new Promise((resolve) => setTimeout(resolve, 100));
        state = {
          ...state,
          selected: question.key,
          snapshot: {
            ...state.snapshot,
            revision: state.snapshot.revision + 1,
            requests: state.snapshot.requests.filter(
              (r) => r.key !== request.key,
            ),
          },
        };
        return structuredClone(state);
      }
      return null;
    });
    return clearMocks;
  },
  decorators: [
    (Story) => (
      <LocaleProvider>
        <div
          style={{
            minHeight: 620,
            padding: 32,
            background:
              "radial-gradient(ellipse at 12% 15%, rgba(123, 154, 226, .32), transparent 55%), radial-gradient(ellipse at 45% 85%, rgba(187, 155, 221, .26), transparent 60%)",
          }}
        >
          <div
            style={{
              position: "relative",
              width: 392,
              height: 560,
            }}
          >
            <Story />
          </div>
        </div>
      </LocaleProvider>
    ),
  ],
} satisfies Meta<typeof PetTaskSurface>;
export default meta;
type Story = StoryObj<typeof meta>;
export const ApprovalAndQuestion: Story = {};
export const TaskList: Story = { parameters: { list: true } };
export const Badge: Story = {
  parameters: { list: true, collapsed: true },
};
export const RetryQuestion: Story = { parameters: { failFirst: true } };
export const MultiStepQuestion: Story = {
  parameters: { failFirst: true, wizard: true },
};
