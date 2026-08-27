import { useState, type CSSProperties } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import {
  FolderTree,
  MessageSquare,
  PanelRight,
  Plus,
  RotateCw,
  X,
} from "lucide-react";

type DockKind = "files" | "side" | "runtime";

function HeaderActions({
  kind,
  onSelect,
}: {
  kind: DockKind | null;
  onSelect?: (kind: DockKind | null) => void;
}) {
  const toggle = (next: DockKind) => onSelect?.(kind === next ? null : next);
  return (
    <div className="header-actions">
      <div className="model-picker">
        <button type="button" className="model-picker-trigger">
          <span className="model-picker-icon" aria-hidden>
            G
          </span>
          <span className="model-picker-label">gemini-3.7-flash</span>
          <span className="model-picker-chevron" aria-hidden>⌄</span>
        </button>
      </div>
      <div className="chat-header-tools">
        <button type="button" className="header-icon-btn" aria-label="新对话">
          <Plus size={16} />
        </button>
        <button
          type="button"
          className={`header-icon-btn${kind === "files" ? " is-active" : ""}`}
          aria-label="项目文件"
          aria-pressed={kind === "files"}
          onClick={() => toggle("files")}
        >
          <FolderTree size={16} />
        </button>
        <button
          type="button"
          className={`header-icon-btn${kind === "side" ? " is-active" : ""}`}
          aria-label="旁路对话"
          aria-pressed={kind === "side"}
          onClick={() => toggle("side")}
        >
          <MessageSquare size={16} />
        </button>
        <button
          type="button"
          className={`header-icon-btn${kind === "runtime" ? " is-active" : ""}`}
          aria-label="运行信息"
          aria-pressed={kind === "runtime"}
          onClick={() => toggle("runtime")}
        >
          <PanelRight size={16} />
        </button>
      </div>
    </div>
  );
}

function DockPanel({ kind }: { kind: DockKind }) {
  if (kind === "files") {
    return (
      <aside className="project-files-panel">
        <header className="project-files-header">
          <span className="project-files-heading">
            <FolderTree size={16} />
            <strong>大模型八股文</strong>
          </span>
          <span className="project-files-actions">
            <button type="button" aria-label="刷新">
              <RotateCw size={15} />
            </button>
            <button type="button" aria-label="关闭">
              <X size={15} />
            </button>
          </span>
        </header>
        <div className="project-files-search">筛选文件…</div>
      </aside>
    );
  }

  if (kind === "side") {
    return (
      <aside className="side-chat-panel">
        <header className="side-chat-head">
          <span className="side-chat-mark">
            <MessageSquare size={15} />
          </span>
          <div className="side-chat-title">
            <strong>对话侧栏</strong>
            <span>临时旁路对话</span>
          </div>
          <button type="button" className="side-chat-close" aria-label="关闭">
            <X size={15} />
          </button>
        </header>
      </aside>
    );
  }

  return (
    <aside
      className="chat-right-panel"
      style={{ "--chat-right-panel-width": "360px" } as CSSProperties}
    >
      <div className="chat-right-header">
        <h2 className="chat-right-title">
          <PanelRight size={17} />运行信息
        </h2>
        <button type="button" className="chat-right-close" aria-label="关闭">
          <X size={14} />
        </button>
      </div>
    </aside>
  );
}

function ChatDockLayout({
  kind,
  onSelect,
}: {
  kind: DockKind | null;
  onSelect?: (kind: DockKind | null) => void;
}) {
  const dockWidth =
    kind === "files" ? 320 : kind === "side" ? 384 : kind === "runtime" ? 360 : 0;
  const dockClasses =
    kind === "files"
      ? " has-project-files"
      : kind === "side"
        ? " has-side-chat"
        : kind === "runtime"
          ? " has-chat-right"
          : "";
  const hasDock = kind !== null;
  return (
    <main
      style={{ minHeight: "100vh", display: "flex", background: "var(--shell-bg)" }}
    >
      <section className="content-pane">
        <div
          className={`content-header content-header--chat${hasDock ? " has-right-dock" : ""}`}
          style={{ "--chat-header-right-offset": `${dockWidth}px` } as CSSProperties}
        >
          <div className="content-heading" />
          <HeaderActions kind={kind} onSelect={onSelect} />
        </div>
        <div className="page-body page-body--chat">
          <div
            className={`chat-layout-with-right${hasDock ? " has-right-dock" : ""}${dockClasses}`}
            style={{
              "--project-files-current-width": `${dockWidth}px`,
              "--project-files-width": `${dockWidth}px`,
            } as CSSProperties}
          >
            <div className="chat-main">
              <div className="chat-pane" style={{ padding: 28 }}>
                <strong style={{ color: "var(--ink)" }}>主对话区域</strong>
                <p style={{ color: "var(--ink-mute)" }}>
                  顶部工具组不再覆盖右侧停靠面板。
                </p>
              </div>
            </div>
            {kind && <DockPanel kind={kind} />}
          </div>
        </div>
      </section>
    </main>
  );
}

function UnifiedDockPreview() {
  const [kind, setKind] = useState<DockKind | null>("files");
  return <ChatDockLayout kind={kind} onSelect={setKind} />;
}

const meta = {
  title: "Chat/Dock Layout",
  component: ChatDockLayout,
  args: { kind: "files" },
  parameters: { controls: { disable: true }, layout: "fullscreen" },
} satisfies Meta<typeof ChatDockLayout>;

export default meta;
type Story = StoryObj<typeof meta>;

export const ProjectFiles: Story = {};
export const SideChat: Story = { args: { kind: "side" } };
export const RuntimePanel: Story = { args: { kind: "runtime" } };
export const UnifiedSwitching: Story = { render: () => <UnifiedDockPreview /> };
