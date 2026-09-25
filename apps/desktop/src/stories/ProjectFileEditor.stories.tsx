import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState, type CSSProperties } from "react";
import WorkspaceEditor from "../components/workspace/WorkspaceEditor";
import type { ResolvedTheme } from "../hooks/app/useTheme";

const identitySample = `# IDENTITY.md — Agent 身份

描述这个 Agent 是谁、服务什么目标；详细工作方式见 AGENTS.md，表达风格见 SOUL.md。

- **Name:** Astro
- **Id:** default
- **Role:** AI 助手与数字搭档
- **Scope:** 在用户授权范围内协作，不代替用户作未经授权的重要决定

## 职责

- 理解用户目标，提供有依据的回答，或完成受托的实施与交付
- 帮助用户管理知识、工具和任务，不把临时进展混入长期记忆
- 说明重要取舍、能力边界与尚未验证的部分
`;

/** 项目文件编辑器：点进某一行后不应留下整行高亮色带。 */
function ProjectFileEditorSample({
  theme = "light",
}: {
  theme?: ResolvedTheme;
}) {
  const [value, setValue] = useState(identitySample);
  return (
    <div
      className="app-shell"
      data-tone="blue"
      style={
        {
          height: 460,
          padding: 12,
          background: "var(--shell-bg)",
        } as CSSProperties
      }
    >
      <section className="project-file-workbench" aria-label="文件编辑器">
        <div className="project-file-editor-body">
          <WorkspaceEditor
            value={value}
            filename="IDENTITY.md"
            theme={theme}
            onChange={setValue}
          />
        </div>
      </section>
    </div>
  );
}

const meta = {
  title: "Design/Project File Editor",
  component: ProjectFileEditorSample,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof ProjectFileEditorSample>;
export default meta;
type Story = StoryObj<typeof meta>;

export const MarkdownSource: Story = {};

export const MarkdownSourceDark: Story = {
  args: { theme: "dark" },
};
