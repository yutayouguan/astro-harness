import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { Bot, FileText, Sparkles } from "lucide-react";
import ComposerContextPreview, {
  type ComposerPreviewTarget,
} from "../components/chat/ComposerContextPreview";
import McpIcon from "../components/icons/McpIcon";
import { LocaleProvider } from "../i18n/LocaleContext";
import type { ComposerContextToken } from "../lib/chat/composerContext";
import type { ChatAttachment } from "../types";

const attachment: ChatAttachment = {
  id: "sample-file",
  name: "产品需求说明.md",
  mime: "text/markdown",
  kind: "file",
  size: 2840,
  dataBase64: btoa("# Product brief\n\nThis text attachment supports an inline preview."),
};

const contexts: ComposerContextToken[] = [
  {
    id: "skill-documents",
    kind: "skill",
    name: "documents",
    description: "创建、编辑和审阅文档",
    path: "/Users/demo/.codex/skills/documents/SKILL.md",
  },
  {
    id: "mcp-browser",
    kind: "mcp",
    name: "browser",
    description: "访问并检查浏览器页面",
  },
  {
    id: "agent-reviewer",
    kind: "agent",
    name: "reviewer",
    description: "审阅实现和交互细节",
  },
];

function TokenIcon({ kind }: { kind: ComposerContextToken["kind"] }) {
  if (kind === "skill") return <Sparkles size={15} aria-hidden />;
  if (kind === "mcp") return <McpIcon size={15} />;
  return <Bot size={15} aria-hidden />;
}

function ComposerContextTokens() {
  const [preview, setPreview] = useState<ComposerPreviewTarget | null>(null);

  return (
    <LocaleProvider>
      <main
        style={{
          minHeight: "100vh",
          display: "grid",
          placeItems: "center",
          padding: 32,
          boxSizing: "border-box",
          background: "var(--shell-bg)",
        }}
      >
        <div
          className="composer composer--stacked"
          style={{ width: "min(760px, calc(100vw - 48px))" }}
        >
          <div className="composer-previews composer-context-strip">
            <div className="composer-preview" data-kind="file">
              <button
                type="button"
                className="composer-preview-open"
                onClick={() => setPreview({ type: "attachment", item: attachment })}
              >
                <span className="composer-preview-icon" data-kind="file">
                  <FileText size={16} aria-hidden />
                </span>
                <span className="composer-preview-meta">
                  <span className="composer-preview-name">{attachment.name}</span>
                  <span className="composer-preview-size">2.8 KB</span>
                </span>
              </button>
              <button type="button" className="composer-preview-remove">×</button>
            </div>
            {contexts.map((token) => (
              <div
                key={token.id}
                className="composer-context-token"
                data-kind={token.kind}
              >
                <button
                  type="button"
                  className="composer-context-token-open"
                  onClick={() => setPreview({ type: "context", item: token })}
                >
                  <span className="composer-context-token-icon">
                    <TokenIcon kind={token.kind} />
                  </span>
                  <span className="composer-context-token-meta">
                    <span className="composer-context-token-kind">{token.kind}</span>
                    <span className="composer-context-token-name">{token.name}</span>
                  </span>
                </button>
                <button type="button" className="composer-preview-remove">×</button>
              </div>
            ))}
          </div>
          <textarea
            className="composer-input"
            defaultValue="请结合这些上下文优化方案"
            aria-label="消息"
          />
        </div>
        <ComposerContextPreview
          target={preview}
          onClose={() => setPreview(null)}
        />
      </main>
    </LocaleProvider>
  );
}

const meta = {
  title: "Chat/Composer Context Tokens",
  component: ComposerContextTokens,
  parameters: { controls: { disable: true }, layout: "fullscreen" },
} satisfies Meta<typeof ComposerContextTokens>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
