import { useState, type CSSProperties } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import {
  Activity,
  Eye,
  FileText,
  FolderTree,
  Gauge,
  GitBranch,
  Layers,
  MessageSquare,
  PanelRight,
  Plus,
  RotateCw,
  Save,
  SendHorizontal,
  X,
} from "lucide-react";
import SubagentActivityBar from "../components/chat/SubagentActivityBar";
import TaskMonitorPanel from "../components/chat/TaskMonitorPanel";
import ConversationTitle from "../components/chat/ConversationTitle";
import { LocaleProvider } from "../i18n/LocaleContext";
import { MorphiconProvider } from "../hooks/app/useMorphicons";
import { useBeautifyTips } from "../hooks/ui/useBeautifyTips";

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
          <span className="model-picker-chevron" aria-hidden>
            ⌄
          </span>
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
          className={`header-icon-btn header-summary-btn${kind === "runtime" ? " is-active" : ""}`}
          aria-label="运行摘要，2 个 Subagent 运行中"
          aria-pressed={kind === "runtime"}
          onClick={() => toggle("runtime")}
        >
          <Activity size={16} />
          <span className="header-summary-badge" aria-hidden>
            2
          </span>
        </button>
      </div>
    </div>
  );
}

function DockPanel({ kind }: { kind: DockKind }) {
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
        <div className="side-chat-body">
          <section className="chat-pane">
            <div className="message-list-wrap">
              <div className="message-list">
                <article className="msg-row assistant">
                  <div className="avatar is-model">G</div>
                  <div className="msg-stack">
                    <div className="bubble assistant">
                      <div className="msg-content">
                        <p>
                          当前任务已完成界面梳理，我可以继续检查工具调用和审批流程。
                        </p>
                      </div>
                    </div>
                  </div>
                </article>
                <article className="msg-row user">
                  <div className="msg-stack">
                    <div className="bubble user">
                      <div className="msg-content">
                        再检查一下 Skills 和附件
                      </div>
                    </div>
                  </div>
                </article>
              </div>
            </div>
            <form className="composer-shell">
              <div className="composer composer--stacked">
                <div className="composer-input-wrap">
                  <textarea
                    className="composer-input"
                    placeholder="询问当前任务…"
                    rows={1}
                  />
                </div>
                <div className="composer-bar">
                  <div className="composer-bar-left">
                    <button type="button" className="composer-mode-pill">
                      ∞ Agent
                    </button>
                    <button
                      type="button"
                      className="composer-icon-btn"
                      aria-label="添加"
                    >
                      <Plus size={17} />
                    </button>
                  </div>
                  <div className="composer-bar-right">
                    <button
                      type="button"
                      className="send-btn send-btn--round"
                      aria-label="发送"
                    >
                      <SendHorizontal size={15} />
                    </button>
                  </div>
                </div>
              </div>
            </form>
          </section>
        </div>
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
          <PanelRight size={17} />
          会话详情
        </h2>
        <button type="button" className="chat-right-close" aria-label="关闭">
          <X size={14} />
        </button>
      </div>
      <div className="chat-right-tabs" role="tablist">
        <button
          type="button"
          role="tab"
          aria-selected
          className="chat-right-tab is-active"
        >
          <Activity size={15} />
          运行摘要
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={false}
          className="chat-right-tab"
        >
          <Layers size={15} />
          上下文
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={false}
          className="chat-right-tab"
        >
          <GitBranch size={15} />
          分支
        </button>
      </div>
      <div className="chat-right-body">
        <LocaleProvider>
          <MorphiconProvider>
            <div className="chat-summary-panel">
              <div className="chat-agent-info">
                <header className="chat-agent-hero">
                  <div className="chat-agent-avatar" aria-hidden>
                    A
                  </div>
                  <div className="chat-agent-identity">
                    <h3>Astro</h3>
                    <p className="chat-agent-status">官方默认 Agent</p>
                  </div>
                </header>
                <section className="chat-agent-card">
                  <h4 className="chat-agent-card-title">
                    <span className="chat-agent-card-icon" aria-hidden>
                      <Gauge size={15} />
                    </span>
                    用量参考
                  </h4>
                  <p className="chat-agent-usage-empty muted">
                    本轮暂无用量数据
                  </p>
                </section>
              </div>
              <SubagentActivityBar
                rootSessionId="story-session"
                roots={[]}
                showEmpty
                onRefresh={async () => {}}
                onOpenPanel={() => {}}
                onOpenThread={() => {}}
              />
              <TaskMonitorPanel messages={[]} streaming={false} />
            </div>
          </MorphiconProvider>
        </LocaleProvider>
      </div>
    </aside>
  );
}

function ProjectFilesDockPreview({ open }: { open: boolean }) {
  return (
    <aside className={`project-files-panel${open ? " is-open" : ""}`}>
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
      <div className="project-files-tree">
        <button type="button" className="project-file-row">
          <span className="project-file-chevron">⌄</span>
          <FolderTree size={15} />
          <span className="project-file-name">plans</span>
        </button>
        <button
          type="button"
          className="project-file-row is-active"
          style={{ "--tree-level": 1 } as CSSProperties}
        >
          <span className="project-file-chevron-spacer" />
          <FileText size={14} />
          <span className="project-file-name">20260830-f28f42.md</span>
        </button>
        <button type="button" className="project-file-row">
          <span className="project-file-chevron">›</span>
          <FolderTree size={15} />
          <span className="project-file-name">skills</span>
        </button>
      </div>
    </aside>
  );
}

function ProjectFileWorkbenchPreview() {
  return (
    <section className="project-file-workbench">
      <div className="project-file-tabs">
        <div className="project-file-tabs-scroll">
          <div className="project-file-tab">
            <FileText size={14} />
            <span>20260829-2db8a3.md</span>
            <span className="project-file-tab-close">×</span>
          </div>
          <div className="project-file-tab is-active">
            <FileText size={14} />
            <span>20260830-f28f42.md</span>
            <span className="project-file-tab-close">×</span>
          </div>
        </div>
        <div className="project-file-tab-actions">
          <button type="button" aria-label="预览">
            <Eye size={15} />
          </button>
          <button type="button" aria-label="保存">
            <Save size={15} />
          </button>
          <button type="button" aria-label="关闭">
            <X size={15} />
          </button>
        </div>
      </div>
      <div className="project-file-editor-body">
        <div
          aria-label="Markdown 编辑器预览"
          style={{
            display: "grid",
            gridTemplateColumns: "32px minmax(0, 1fr)",
            gap: "10px 14px",
            padding: "18px 24px",
            color: "var(--ink-soft)",
            fontFamily: "var(--font-mono)",
            fontSize: 13,
            lineHeight: 1.55,
          }}
        >
          <span style={{ color: "var(--ink-mute)", textAlign: "right" }}>
            1
          </span>
          <strong># 简单网页标题爬虫</strong>
          <span style={{ color: "var(--ink-mute)", textAlign: "right" }}>
            2
          </span>
          <span />
          <span style={{ color: "var(--ink-mute)", textAlign: "right" }}>
            3
          </span>
          <span>- [ ] 编写抓取页面标题的脚本</span>
          <span style={{ color: "var(--ink-mute)", textAlign: "right" }}>
            4
          </span>
          <span>- [ ] 实测脚本并整理输出标题</span>
        </div>
      </div>
    </section>
  );
}

function ChatDockLayout({
  kind,
  onSelect,
}: {
  kind: DockKind | null;
  onSelect?: (kind: DockKind | null) => void;
}) {
  useBeautifyTips();
  const dockWidth =
    kind === "files"
      ? 320
      : kind === "side"
        ? 384
        : kind === "runtime"
          ? 360
          : 0;
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
      style={{
        minHeight: "100vh",
        display: "flex",
        background: "var(--shell-bg)",
      }}
    >
      <section className="content-pane content-pane--chat">
        <div
          className={`content-header content-header--chat${hasDock ? " has-right-dock" : ""}`}
        >
          <div className="content-heading">
            <div className="page-title-block">
              <div className="page-title-icon" data-tone="blue" aria-hidden>
                <MessageSquare size={15} />
              </div>
              <div className="page-title-text">
                <h1
                  className="content-title conversation-title"
                  data-tone="blue"
                >
                  <ConversationTitle
                    title="请用 Python 写一个简单的 Web 爬虫，抓取页面标题并整理运行结果"
                    renameLabel="重命名"
                    onRename={() => {}}
                  />
                </h1>
              </div>
            </div>
          </div>
          <HeaderActions kind={kind} onSelect={onSelect} />
        </div>
        <div className="page-body page-body--chat">
          <div
            className={`chat-layout-with-right${hasDock ? " has-right-dock" : ""}${dockClasses}`}
            style={
              {
                "--project-files-current-width": `${dockWidth}px`,
                "--project-files-width": `${dockWidth}px`,
              } as CSSProperties
            }
          >
            <div className="chat-main">
              <div
                className={`chat-pane${kind === "files" ? " has-project-file" : ""}`}
              >
                {kind === "files" ? (
                  <ProjectFileWorkbenchPreview />
                ) : (
                  <div style={{ padding: 28 }}>
                    <strong style={{ color: "var(--ink)" }}>主对话区域</strong>
                    <p style={{ color: "var(--ink-mute)" }}>
                      顶部工具组不再覆盖右侧停靠面板。
                    </p>
                  </div>
                )}
                {kind === "files" ? (
                  <form className="composer-shell">
                    <div className="composer composer--stacked">
                      <div className="composer-input-wrap">
                        <textarea
                          className="composer-input"
                          placeholder="询问或编辑当前文件…"
                          rows={1}
                        />
                      </div>
                      <div className="composer-bar">
                        <div className="composer-bar-left">
                          <button type="button" className="composer-mode-pill">
                            ∞ Agent
                          </button>
                          <button
                            type="button"
                            className="composer-icon-btn"
                            aria-label="添加"
                          >
                            <Plus size={17} />
                          </button>
                        </div>
                        <div className="composer-bar-right">
                          <button
                            type="button"
                            className="send-btn send-btn--round"
                            aria-label="发送"
                          >
                            <SendHorizontal size={15} />
                          </button>
                        </div>
                      </div>
                    </div>
                  </form>
                ) : null}
              </div>
            </div>
            <ProjectFilesDockPreview open={kind === "files"} />
            <div
              className={`side-chat-dock${kind === "side" ? " is-open" : ""}`}
            >
              {kind === "side" ? <DockPanel kind="side" /> : null}
            </div>
            {kind === "runtime" ? <DockPanel kind={kind} /> : null}
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
