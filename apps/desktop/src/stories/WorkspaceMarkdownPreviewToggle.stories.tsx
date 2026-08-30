import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { LocaleProvider } from "../i18n/LocaleContext";
import { MorphiconProvider } from "../hooks/app/useMorphicons";
import { MarkdownPreviewToggle } from "../components/workspace/WorkspacePanel";

function PreviewToggleStory() {
  const [preview, setPreview] = useState(false);

  return (
    <MorphiconProvider>
      <LocaleProvider>
        <main
          style={{
            minHeight: "100vh",
            display: "grid",
            placeItems: "center",
            background: "var(--shell-bg)",
          }}
        >
          <MarkdownPreviewToggle
            preview={preview}
            onToggle={() => setPreview((current) => !current)}
          />
        </main>
      </LocaleProvider>
    </MorphiconProvider>
  );
}

const meta = {
  id: "workspace-markdown-preview-toggle",
  title: "Workspace/Markdown Preview Toggle",
  component: PreviewToggleStory,
  parameters: { controls: { disable: true } },
} satisfies Meta<typeof PreviewToggleStory>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
