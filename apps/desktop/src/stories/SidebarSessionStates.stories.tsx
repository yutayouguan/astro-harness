import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { Meta, StoryObj } from "@storybook/react-vite";
import {
  Archive,
  ChevronRight,
  Cpu,
  FolderTree,
  History,
  Layers2,
  MessageSquare,
  MoreVertical,
  Pin,
  Plus,
  Wrench,
} from "lucide-react";
import { useState, type CSSProperties } from "react";
import ProjectFolderIcon from "../components/chat/ProjectFolderIcon";
import SidebarSessionList from "../components/chat/SidebarSessionList";
import SessionStatusIcon from "../components/chat/SessionStatusIcon";
import { AstroLogoMark } from "../components/icons/AstroLogoMark";
import {
  IconCron,
  IconLoop,
  IconNewChat,
  IconPlugin,
  IconSettings,
} from "../components/icons/NavIcons";
import { MorphiconProvider } from "../hooks/app/useMorphicons";
import { DialogProvider } from "../hooks/ui/DialogContext";
import ExpandableSearch from "../components/ui/ExpandableSearch";
import { useBeautifyTips } from "../hooks/ui/useBeautifyTips";
import { LocaleProvider } from "../i18n/LocaleContext";
import type { RecentSessionDto } from "../types";

const menuSessions = Array.from({ length: 5 }, (_, index) => ({
  sessionId: `menu-session-${index + 1}`,
  source: "tauri",
  summary:
    index === 0
      ? "生成一张党政风格的 PPT 封面背景，包含红色丝带与城市剪影"
      : `生成一张党政风格的 PPT 封面背景 ${index + 1}`,
  createdAt: new Date(Date.now() - index * 3_600_000).toISOString(),
  endReason: null,
  archivedAt: null,
  pinnedAt: null,
})) satisfies RecentSessionDto[];

/** 定时任务会话的存储标题带「定时任务 · 」前缀，展示层换成时钟图标。 */
const cronAwareSessions = [
  {
    sessionId: "cron-session-daily",
    source: "tauri",
    summary: "定时任务 · 每日 8 点舆情早报",
    createdAt: new Date().toISOString(),
    endReason: null,
    archivedAt: null,
    pinnedAt: null,
  },
  {
    sessionId: "737536f4-6588-4cbc-99a2-7abc20d49b15",
    source: "tauri",
    summary: "定时任务 · 每周一整理收件箱 · 737536f4",
    createdAt: new Date(Date.now() - 3_600_000).toISOString(),
    endReason: null,
    archivedAt: null,
    pinnedAt: null,
  },
  {
    sessionId: "chat-session-1",
    source: "tauri",
    summary: "美化 AI 回答面板",
    createdAt: new Date(Date.now() - 7_200_000).toISOString(),
    endReason: null,
    archivedAt: null,
    pinnedAt: null,
  },
] satisfies RecentSessionDto[];

function SessionRow({
  title,
  time,
  status,
  unread = false,
  active = false,
}: {
  title: string;
  time?: string;
  status: "idle" | "running" | "awaiting" | "error";
  unread?: boolean;
  active?: boolean;
}) {
  return (
    <div
      className={`sidebar-session-item ${active ? "is-active" : ""} ${unread ? "is-unread" : ""}`.trim()}
    >
      <button
        type="button"
        className="sidebar-session-main"
        aria-current={active ? "page" : undefined}
      >
        <span className="sidebar-session-title-wrap">
          <span className="sidebar-session-title">{title}</span>
        </span>
        {time ? <span className="sidebar-session-time">{time}</span> : null}
        <SessionStatusIcon
          status={status}
          unread={unread}
          label={
            status === "running" ? "生成中" : unread ? "已完成，未读" : "已完成"
          }
        />
      </button>
      <div className="sidebar-session-actions">
        <button
          type="button"
          className="sidebar-session-action-btn is-pin"
          title="置顶"
          aria-label="置顶"
        >
          <Pin size={13} aria-hidden />
        </button>
        <button
          type="button"
          className="sidebar-session-action-btn is-more"
          title="更多操作"
          aria-label="更多操作"
        >
          <MoreVertical size={13} aria-hidden />
        </button>
      </div>
    </div>
  );
}

function SidebarSessionStates({ labels = true }: { labels?: boolean }) {
  useBeautifyTips();
  const [query, setQuery] = useState("");
  const [mainProjectExpanded, setMainProjectExpanded] = useState(true);
  const [railPreview, setRailPreview] = useState(false);
  const showLabels = labels || railPreview;

  return (
    <main className="app-shell" data-tone="blue" style={{ minHeight: "100vh" }}>
      <div className="body-row">
        <aside
          className={`sidebar is-open is-pinned ${showLabels ? "is-labels" : "is-icons"} ${railPreview ? "is-rail-preview" : ""}`}
          style={{ "--sidebar-w-wide": "280px" } as CSSProperties}
          onMouseLeave={() => setRailPreview(false)}
        >
          <div className="sidebar-brand">
            <div className="sidebar-logo" aria-hidden>
              <AstroLogoMark width={26} height={26} />
            </div>
            <div className="sidebar-brand-text">Astro Harness</div>
          </div>
          <div className="sidebar-primary-actions">
            <button type="button" className="sidebar-new-chat">
              <IconNewChat width={18} height={18} strokeWidth={1.8} />
              <span className="sidebar-item-label">新对话</span>
              <kbd className="sidebar-new-chat-shortcut" aria-hidden>
                ⌘N
              </kbd>
            </button>
            <ExpandableSearch
              value={query}
              onChange={setQuery}
              placeholderKey="chat.rightPanel.searchSessions"
              className="sidebar-session-search sidebar-global-search"
            />
          </div>
          <div className="sidebar-group-label">工作台</div>
          <nav className="sidebar-feature-tabs" aria-label="自动化与扩展">
            {[
              { label: "定时任务", Icon: IconCron },
              { label: "智能流程", Icon: IconLoop },
              { label: "插件", Icon: IconPlugin },
            ].map(({ label, Icon }) => (
              <button key={label} type="button" className="sidebar-feature-tab">
                <Icon width={18} height={18} strokeWidth={1.8} />
                <span className="sidebar-item-label">{label}</span>
              </button>
            ))}
          </nav>
          <div className="sidebar-projects">
            <div className="sidebar-collapsible-section">
              <button
                type="button"
                className="sidebar-section-toggle"
                aria-expanded
                aria-label="项目"
                title={showLabels ? undefined : "项目"}
                onClick={() => setRailPreview(true)}
              >
                <span className="sidebar-section-icon" aria-hidden>
                  <FolderTree size={18} strokeWidth={1.8} />
                </span>
                <span className="sidebar-section-title">项目</span>
                <ChevronRight
                  className="sidebar-section-chevron is-expanded"
                  size={12}
                  aria-hidden
                />
              </button>
              <div className="sidebar-section-actions">
                <button type="button" className="sidebar-add-btn" title="新建">
                  <Plus size={14} aria-hidden />
                </button>
              </div>
            </div>
            <div className="sidebar-project">
              <div className="sidebar-project-header">
                <button
                  type="button"
                  className="sidebar-project-name"
                  aria-expanded={mainProjectExpanded}
                  onClick={() => {
                    if (!showLabels) {
                      setMainProjectExpanded(true);
                      setRailPreview(true);
                    } else {
                      setMainProjectExpanded((expanded) => !expanded);
                    }
                  }}
                >
                  <ProjectFolderIcon
                    iconId="astro-space"
                    expanded={mainProjectExpanded}
                    size={18}
                  />
                  <span className="sidebar-item-label">主空间</span>
                </button>
              </div>
              {mainProjectExpanded ? (
                <div className="sidebar-sessions">
                  <SessionRow
                    title="请用 Python 写一个简单的 Web 爬虫"
                    time="15 小时前"
                    status="idle"
                    active
                  />
                </div>
              ) : null}
            </div>
            <div className="sidebar-project">
              <div className="sidebar-project-header">
                <button type="button" className="sidebar-project-name">
                  <ProjectFolderIcon iconId="folder-rust" expanded size={18} />
                  <span className="sidebar-item-label">大模型八股文</span>
                </button>
              </div>
            </div>
            <div className="sidebar-collapsible-section">
              <button
                type="button"
                className="sidebar-section-toggle"
                aria-expanded
                aria-label="最近"
                title={showLabels ? undefined : "最近"}
                onClick={() => setRailPreview(true)}
              >
                <span className="sidebar-section-icon" aria-hidden>
                  <History size={18} strokeWidth={1.8} />
                </span>
                <span className="sidebar-section-title">最近</span>
                <ChevronRight
                  className="sidebar-section-chevron is-expanded"
                  size={12}
                  aria-hidden
                />
              </button>
              <div className="sidebar-section-actions">
                <button
                  type="button"
                  className="sidebar-session-filter-btn"
                  title="已归档"
                  aria-pressed="false"
                >
                  <Archive size={14} strokeWidth={1.8} aria-hidden />
                </button>
              </div>
            </div>
            <div className="sidebar-sessions is-global">
              <SessionRow title="统一聊天 AI 卡片样式" status="running" />
              <SessionRow title="美化 AI 回答面板" status="running" />
              <SessionRow
                title="输入框的上下文量显示按钮呢"
                status="idle"
                unread
              />
              <SessionRow title="已读的历史任务" status="idle" time="2 天前" />
            </div>
          </div>
          <div className="sidebar-footer">
            <button type="button" className="sidebar-settings-btn">
              <IconSettings width={17} height={17} strokeWidth={1.8} />
              <span className="sidebar-item-label">偏好设置</span>
            </button>
          </div>
        </aside>
      </div>
    </main>
  );
}

function SettingsMenu() {
  const items = [
    { label: "对话", Icon: MessageSquare },
    { label: "自动压缩", Icon: Layers2 },
    { label: "模型服务", Icon: Cpu, active: true },
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

function SessionMenuNearBottom() {
  useBeautifyTips();

  return (
    <main
      className="app-shell"
      data-tone="blue"
      style={{ width: 620, height: 540 }}
    >
      <div className="body-row">
        <aside
          className="sidebar is-open is-pinned is-labels"
          style={{ "--sidebar-w-wide": "360px" } as CSSProperties}
        >
          <div style={{ marginTop: "auto" }}>
            <SidebarSessionList
              activeSessionId="menu-session-5"
              sessionStatuses={{}}
              projectId={null}
              query=""
              listKind="active"
              onOpenSession={() => {}}
            />
          </div>
        </aside>
      </div>
    </main>
  );
}

function CronSessionTitles() {
  return (
    <main className="app-shell" data-tone="blue" style={{ minHeight: "100vh" }}>
      <div className="body-row">
        <aside
          className="sidebar is-open is-pinned is-labels"
          style={{ "--sidebar-w-wide": "320px" } as CSSProperties}
        >
          <div className="sidebar-sessions is-global">
            <SidebarSessionList
              activeSessionId={null}
              sessionStatuses={{}}
              projectId={null}
              query=""
              listKind="active"
              onOpenSession={() => {}}
            />
          </div>
        </aside>
      </div>
    </main>
  );
}

/** 定时任务会话的归属标注：任务在调度中、已归档、已被删除。 */
const cronOwnerSessions = [
  {
    sessionId: "cron-job-active",
    source: "tauri",
    summary: "定时任务 · 每日 8 点舆情早报",
    createdAt: new Date().toISOString(),
    endReason: null,
    archivedAt: null,
    pinnedAt: null,
  },
  {
    sessionId: "cron-job-archived",
    source: "tauri",
    summary: "定时任务 · 归档的周报",
    createdAt: new Date(Date.now() - 3_600_000).toISOString(),
    endReason: null,
    archivedAt: null,
    pinnedAt: null,
  },
  {
    sessionId: "cron-job-gone",
    source: "tauri",
    summary: "定时任务 · 已删除的备份任务",
    createdAt: new Date(Date.now() - 7_200_000).toISOString(),
    endReason: null,
    archivedAt: null,
    pinnedAt: null,
  },
] satisfies RecentSessionDto[];

const cronOwnerJobs = [
  { id: "job-active", archived_at: null },
  { id: "job-archived", archived_at: "2026-09-20T10:00:00+08:00" },
];

const meta = {
  id: "sidebar-session-states",
  title: "Shell/Sidebar Session States",
  component: SidebarSessionStates,
  beforeEach: () => {
    mockIPC((command) => {
      if (command === "list_sessions") return menuSessions;
      return null;
    });
    return () => clearMocks();
  },
  decorators: [
    (Story) => (
      <LocaleProvider>
        <MorphiconProvider>
          <DialogProvider>
            <Story />
          </DialogProvider>
        </MorphiconProvider>
      </LocaleProvider>
    ),
  ],
  parameters: {
    controls: { disable: true },
  },
} satisfies Meta<typeof SidebarSessionStates>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const CollapsedNavigation: Story = {
  args: { labels: false },
};

export const SettingsNavigation: Story = {
  render: () => <SettingsMenu />,
};

export const SessionMenuNearBottomEdge: Story = {
  render: () => <SessionMenuNearBottom />,
};

export const CronSessionTitlesStory: Story = {
  name: "Cron Session Titles",
  render: () => <CronSessionTitles />,
  beforeEach: () => {
    mockIPC((command) =>
      command === "list_sessions" ? cronAwareSessions : null,
    );
    return () => clearMocks();
  },
};

export const CronOwnerStatesStory: Story = {
  name: "Cron Owner States",
  render: () => <CronSessionTitles />,
  beforeEach: () => {
    mockIPC((command) => {
      if (command === "list_sessions") return cronOwnerSessions;
      if (command === "list_cron_jobs") return cronOwnerJobs;
      return null;
    });
    return () => clearMocks();
  },
};
