import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import ComposerClarifySurface from "../components/chat/ComposerClarifySurface";
import { LocaleProvider } from "../i18n/LocaleContext";
import type { UiSurface } from "../types";

const steps = [
  {
    id: "goal",
    question: "你的目标时间与减重幅度是多少？",
    options: ["8周减8kg", "12周减6kg", "6周减3kg", "16周减10kg"],
  },
  {
    id: "role",
    question: "目标岗位是哪个方向？",
    options: ["后端开发工程师", "前端开发工程师", "算法/AI 工程师", "产品经理"],
  },
];

const surface: UiSurface = {
  messageId: "clarify-preview",
  activityType: "clarify",
  status: "active",
  operations: [
    {
      createSurface: {
        surfaceId: "clarify-preview",
        catalogId: "astro://a2ui/catalog/v2",
      },
    },
    {
      updateComponents: {
        surfaceId: "clarify-preview",
        components: [
          { id: "root", component: "Card", child: "col" },
          {
            id: "col",
            component: "Column",
            children: ["badge", "title", "wizard"],
          },
          { id: "badge", component: "Badge", text: "Clarify", variant: "info" },
          {
            id: "title",
            component: "Text",
            text: "定制专属健身计划 - 基础信息收集",
            variant: "h2",
          },
          { id: "wizard", component: "ClarifyWizard", steps },
        ],
      },
    },
  ],
};

const approvalSurface: UiSurface = {
  messageId: "approval-preview",
  activityType: "confirmation",
  status: "active",
  operations: [
    {
      createSurface: {
        surfaceId: "approval-preview",
        catalogId: "astro://a2ui/catalog/v2",
      },
    },
    {
      updateComponents: {
        surfaceId: "approval-preview",
        components: [
          { id: "root", component: "Card", child: "col" },
          { id: "col", component: "Column", children: ["wizard"] },
          {
            id: "wizard",
            component: "ClarifyWizard",
            variant: "approval",
            title: "批准危险命令",
            body: '检测到潜在危险操作（dynamic shell expansion）：\n\n```sh\nUA="aihot-skill/0.3.6 (+https://aihot.virxact.com/aihot-skill/)" curl -sS --max-time 20 -H "User-Agent: $UA" "https://aihot.virxact.com/api/public/items?mode=selected&take=10"\n```',
            allowAlways: true,
            approvalTypeLabel: "curl",
            steps: [
              {
                id: "confirm",
                question: "批准危险命令",
                options: ["approve", "deny", "approve_always", "approve_type"],
              },
            ],
          },
        ],
      },
    },
  ],
};

function ClarifyWizardPreview({
  previewSurface = surface,
}: {
  previewSurface?: UiSurface;
}) {
  const [result, setResult] = useState<Record<string, unknown> | null>(null);

  return (
    <LocaleProvider>
      <main
        data-testid="clarify-wizard-preview"
        style={{
          minHeight: "100vh",
          boxSizing: "border-box",
          padding: "48px 24px",
          display: "flex",
          flexDirection: "column",
          justifyContent: "flex-end",
          background: "var(--shell-bg)",
          color: "var(--ink)",
        }}
      >
        <div className="composer-shell">
          <div className="composer composer--stacked has-clarify">
            <ComposerClarifySurface
              surface={previewSurface}
              onAction={(_, context) => setResult(context)}
            />
            <div className="composer-bar" aria-hidden>
              <div className="composer-bar-left">
                <span className="composer-mode-pill">∞ Agent</span>
              </div>
              <div className="composer-bar-right" />
            </div>
          </div>
          {result ? (
            <pre data-testid="clarify-result" style={{ marginTop: 20 }}>
              {JSON.stringify(result, null, 2)}
            </pre>
          ) : null}
        </div>
      </main>
    </LocaleProvider>
  );
}

const meta = {
  id: "clarify-wizard",
  title: "Chat/Clarify Wizard",
  component: ClarifyWizardPreview,
  parameters: {
    controls: { disable: true },
  },
} satisfies Meta<typeof ClarifyWizardPreview>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
export const Approval: Story = {
  render: () => <ClarifyWizardPreview previewSurface={approvalSurface} />,
};

// 与 `a2ui::templates::build_network_approval_surface` 同形：网络授权走同一张卡，
// 只是换成 allow_once / allow_session / allow_always / deny 四个作用域动作。
const networkApprovalSurface: UiSurface = {
  messageId: "network-approval-preview",
  activityType: "confirmation",
  status: "active",
  operations: [
    {
      createSurface: {
        surfaceId: "network-approval-preview",
        catalogId: "astro://a2ui/catalog/v2",
      },
    },
    {
      updateComponents: {
        surfaceId: "network-approval-preview",
        components: [
          { id: "root", component: "Card", child: "col" },
          { id: "col", component: "Column", children: ["wizard"] },
          {
            id: "wizard",
            component: "ClarifyWizard",
            variant: "approval",
            approvalKind: "network",
            approvalDetail: "https://api.virxact.com:8443",
            approvalHost: "api.virxact.com",
            approvalProfile: "aihot",
            approvalCommand:
              'curl -sS --max-time 20 -H "User-Agent: aihot-skill/0.3.6" "https://api.virxact.com/api/public/items?mode=selected&take=10"',
            allowSession: true,
            allowAlways: true,
            approvalTypeLabel: "api.virxact.com",
            steps: [
              {
                id: "network",
                question: "network_approval",
                options: [
                  "allow_once",
                  "allow_session",
                  "allow_always",
                  "deny",
                ],
              },
            ],
          },
        ],
      },
    },
  ],
};

export const ApprovalNetwork: Story = {
  render: () => <ClarifyWizardPreview previewSurface={networkApprovalSurface} />,
};

// 与 `a2ui::templates::build_sandbox_retry_surface` 同形：沙箱拒绝后的一次性提权授权。
const sandboxRetrySurface: UiSurface = {
  messageId: "sandbox-retry-preview",
  activityType: "confirmation",
  status: "active",
  operations: [
    {
      createSurface: {
        surfaceId: "sandbox-retry-preview",
        catalogId: "astro://a2ui/catalog/v2",
      },
    },
    {
      updateComponents: {
        surfaceId: "sandbox-retry-preview",
        components: [
          { id: "root", component: "Card", child: "col" },
          { id: "col", component: "Column", children: ["wizard"] },
          {
            id: "wizard",
            component: "ClarifyWizard",
            variant: "approval",
            approvalKind: "sandbox_retry",
            approvalDetail:
              "sandbox denied write to /Users/me/project/out/report.md (read-only profile)",
            approvalCommand:
              "cat /Users/me/project/in/report.md > /Users/me/project/out/report.md",
            steps: [
              {
                id: "confirm",
                question: "sandbox_retry",
                options: ["approve", "deny"],
              },
            ],
          },
        ],
      },
    },
  ],
};

export const ApprovalSandboxRetry: Story = {
  render: () => <ClarifyWizardPreview previewSurface={sandboxRetrySurface} />,
};

// 与危险命令路径同形：`build_confirm_surface_with_rule(..., risk = Some("dangerous"))`。
const dangerousApprovalSurface: UiSurface = {
  ...approvalSurface,
  messageId: "dangerous-approval-preview",
  operations: [
    {
      createSurface: {
        surfaceId: "dangerous-approval-preview",
        catalogId: "astro://a2ui/catalog/v2",
      },
    },
    {
      updateComponents: {
        surfaceId: "dangerous-approval-preview",
        components: [
          { id: "root", component: "Card", child: "col" },
          { id: "col", component: "Column", children: ["wizard"] },
          {
            id: "wizard",
            component: "ClarifyWizard",
            variant: "approval",
            title: "批准危险命令",
            body:
              '检测到潜在危险操作（dynamic shell expansion）：\n\n```sh\nUA="aihot-skill/0.3.6" curl -sS "https://aihot.virxact.com/api/public/items?mode=selected&take=10"\n```',
            allowAlways: true,
            approvalTypeLabel: "curl",
            approvalRisk: "dangerous",
            steps: [
              {
                id: "confirm",
                question: "批准危险命令",
                options: ["approve", "deny", "approve_always", "approve_type"],
              },
            ],
          },
        ],
      },
    },
  ],
};

export const ApprovalDangerous: Story = {
  render: () => <ClarifyWizardPreview previewSurface={dangerousApprovalSurface} />,
};
