import type { Meta, StoryObj } from "@storybook/react-vite";
import { Archive, ChevronRight, MoreVertical, Pin, Plus } from "lucide-react";
import type { CSSProperties } from "react";
import SessionStatusIcon from "../components/chat/SessionStatusIcon";

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
                <span className="sidebar-section-title">置顶</span>
                <ChevronRight className="sidebar-section-chevron is-expanded" size={12} aria-hidden />
              </button>
              <div className="sidebar-section-actions">
                <button type="button" className="sidebar-add-btn" title="更多">
                  <MoreVertical size={14} aria-hidden />
                </button>
                <button type="button" className="sidebar-add-btn" title="新建">
                  <Plus size={14} aria-hidden />
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
