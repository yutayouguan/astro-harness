import type { Meta, StoryObj } from "@storybook/react-vite";
import { Paperclip, Search, Settings2, Sparkles } from "lucide-react";

function ComposerPlusSearch() {
  return (
    <main
      style={{
        minHeight: "100vh",
        display: "grid",
        placeItems: "center",
        background: "var(--shell-bg)",
      }}
    >
      <section
        className="composer-mcp-menu composer-plus-menu"
        style={{ position: "relative", inset: "auto", visibility: "visible" }}
      >
        <div className="composer-plus-section">
          <div className="composer-mcp-menu-group">添加</div>
          <button type="button" className="composer-plus-action">
            <Paperclip size={16} aria-hidden />
            <span>
              <strong>文件</strong>
              <small>从本机添加附件</small>
            </span>
          </button>
        </div>
        <label className="composer-mcp-menu-search composer-plus-search">
          <Search size={14} strokeWidth={2} aria-hidden />
          <input placeholder="搜索 Skills 或 MCP…" aria-label="搜索 Skills 或 MCP…" />
        </label>
        <div className="composer-mcp-menu-body">
          <div className="composer-mcp-menu-group">插件</div>
          <div className="composer-plus-subgroup-title">
            <Sparkles size={13} aria-hidden />
            Skills
          </div>
        </div>
        <button type="button" className="composer-mcp-menu-footer">
          <Settings2 size={14} aria-hidden />
          打开插件设置
        </button>
      </section>
    </main>
  );
}

const meta = {
  title: "Chat/Composer Plus Search",
  component: ComposerPlusSearch,
  parameters: { controls: { disable: true }, layout: "fullscreen" },
} satisfies Meta<typeof ComposerPlusSearch>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
