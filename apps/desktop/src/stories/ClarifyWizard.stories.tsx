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

function ClarifyWizardPreview() {
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
              surface={surface}
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
