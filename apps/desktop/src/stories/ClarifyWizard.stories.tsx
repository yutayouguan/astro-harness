import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import ClarifyWizard from "../a2ui/ClarifyWizard";
import { LocaleProvider } from "../i18n/LocaleContext";

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
          background: "var(--shell-bg)",
          color: "var(--ink)",
        }}
      >
        <div style={{ width: "min(760px, 100%)", margin: "0 auto" }}>
          <ClarifyWizard steps={steps} onAction={(_, context) => setResult(context)} />
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
