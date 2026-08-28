import type { Meta, StoryObj } from "@storybook/react-vite";
import {
  Archive,
  ChevronRight,
  Cpu,
  Layers2,
  MessageSquare,
  MoreVertical,
  Pin,
  Plus,
  Wrench,
} from "lucide-react";
import type { CSSProperties } from "react";
import ProjectFolderIcon from "../components/chat/ProjectFolderIcon";
import SessionStatusIcon from "../components/chat/SessionStatusIcon";
import { useBeautifyTips } from "../hooks/ui/useBeautifyTips";

function SessionRow({
  title,
  time,
  status,
  unread = false,
}: {
  title: string;
  time?: string;
  status: "idle" | "running" | "awaiting" | "error";
  unread?: boolean;
}) {
  return (
    <div className={`sidebar-session-item ${unread ? "is-unread" : ""}`}>
      <button type="button" className="sidebar-session-main">
        <span className="sidebar-session-title-wrap">
          <span className="sidebar-session-title">{title}</span>
        </span>
        {time ? <span className="sidebar-session-time">{time}</span> : null}
        <SessionStatusIcon
          status={status}
          unread={unread}
          label={status === "running" ? "生成中" : unread ? "已完成，未读" : "已完成"}
        />
      </button>
      <div className="sidebar-session-actions">
        <button type="button" className="sidebar-session-action-btn" title="置顶">
          <Pin size={13} aria-hidden />
        </button>
        <button type="button" className="sidebar-session-action-btn" title="归档">
          <Archive size={13} aria-hidden />
        </button>
        <button type="button" className="sidebar-session-action-btn" title="更多操作">
          <MoreVertical size={13} aria-hidden />
        </button>
      </div>
    </div>
  );
}

function SidebarSessionStates() {
  useBeautifyTips();

  return (
    <main className="app-shell" data-tone="blue" style={{ minHeight: "100vh" }}>
      <div className="body-row">
        <aside
          className="sidebar is-open is-pinned is-labels"
          style={{ "--sidebar-w-wide": "280px" } as CSSProperties}
        >
          <div className="sidebar-projects">
            <div className="sidebar-collapsible-section">
              <button type="button" className="sidebar-section-toggle" aria-expanded>
                <span className="sidebar-section-title">项目</span>
                <ChevronRight className="sidebar-section-chevron is-expanded" size={12} aria-hidden />
              </button>
              <div className="sidebar-section-actions">
                <button
                  type="button"
                  className="sidebar-session-filter-btn is-on"
                  title="已归档"
                  aria-pressed="true"
                >
                  <Archive size={14} strokeWidth={1.8} aria-hidden />
                </button>
                <button type="button" className="sidebar-add-btn" title="新建">
                  <Plus size={14} aria-hidden />
                </button>
              </div>
            </div>
            <div className="sidebar-project">
              <div className="sidebar-project-header">
                <button type="button" className="sidebar-project-name">
                  <ProjectFolderIcon iconId="astro-space" expanded size={18} />
                  <span className="sidebar-item-label">主空间</span>
                </button>
              </div>
            </div>
            <div className="sidebar-project is-active">
              <div className="sidebar-project-header">
                <button type="button" className="sidebar-project-name">
                  <ProjectFolderIcon iconId="folder-rust" expanded size={18} />
                  <span className="sidebar-item-label">大模型八股文</span>
                </button>
              </div>
            </div>
            <div className="sidebar-sessions is-global">
              <SessionRow title="统一聊天 AI 卡片样式" status="running" />
              <SessionRow title="美化 AI 回答面板" status="running" />
              <SessionRow title="输入框的上下文量显示按钮呢" status="idle" unread />
              <SessionRow title="已读的历史任务" status="idle" time="2 天前" />
            </div>
          </div>
        </aside>
      </div>
    </main>
  );
}

function SettingsMenu() {
  const items = [
    { label: "对话", Icon: MessageSquare },
    { label: "上下文与压缩", Icon: Layers2 },
    { label: "模型配置", Icon: Cpu, active: true },
    { label: "工具", Icon: Wrench },
  ];

  return (
    <main className="app-shell" data-tone="pink" style={{ minHeight: "100vh" }}>
      <div className="body-row">
        <aside
          className="sidebar is-open is-pinned is-labels"
          style={{ "--sidebar-w-wide": "280px" } as CSSProperties}
        >
          <div className="sidebar-settings-nav">
            {items.map(({ label, Icon, active }) => (
              <button
                key={label}
                type="button"
                className={`settings-sidebar-item ${active ? "is-active" : ""}`}
              >
                <Icon size={18} strokeWidth={1.6} aria-hidden />
                <span className="sidebar-item-label">{label}</span>
              </button>
            ))}
          </div>
        </aside>
      </div>
    </main>
  );
}

const meta = {
  id: "sidebar-session-states",
  title: "Shell/Sidebar Session States",
  component: SidebarSessionStates,
  parameters: {
    controls: { disable: true },
  },
} satisfies Meta<typeof SidebarSessionStates>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const SettingsNavigation: Story = {
  render: () => <SettingsMenu />,
};
