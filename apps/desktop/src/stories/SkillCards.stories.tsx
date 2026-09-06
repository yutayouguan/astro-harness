import type { Meta, StoryObj } from "@storybook/react-vite";
import {
  Bot,
  CloudDownload,
  Copy,
  Eye,
  FolderOpen,
  HardDrive,
  Link2,
  Package,
  RefreshCw,
} from "lucide-react";

type CardFixture = {
  id: string;
  name: string;
  description: string;
  meta: string;
  state: string;
  stateClass?: string;
  stateKind: string;
  tone: string;
  Icon: typeof Package;
  ActionIcon: typeof Eye;
  action: string;
  secondary: Array<typeof Eye>;
};

const fixtures: CardFixture[] = [
  {
    id: "browser",
    name: "agent-browser",
    description:
      "Browser automation for agents that need to inspect and interact with websites.",
    meta: "个人 · ~/.astro/skills",
    state: "已启用",
    stateClass: "is-on",
    stateKind: "enabled",
    tone: "cyan",
    Icon: Package,
    ActionIcon: Eye,
    action: "查看",
    secondary: [RefreshCw, FolderOpen, Copy],
  },
  {
    id: "local",
    name: "local-workflows",
    description: "来自本机工具目录，可链接到当前 Agent 后按需加载。",
    meta: "~/.agents/skills/local-workflows",
    state: "未链接",
    stateKind: "unlinked",
    tone: "amber",
    Icon: HardDrive,
    ActionIcon: Link2,
    action: "链接",
    secondary: [Eye, FolderOpen, Copy],
  },
  {
    id: "market",
    name: "research-assistant",
    description:
      "Research workflow with source collection, synthesis, and structured reports.",
    meta: "SkillHub",
    state: "1.2k 安装",
    stateKind: "available",
    tone: "purple",
    Icon: CloudDownload,
    ActionIcon: CloudDownload,
    action: "安装",
    secondary: [Bot, Copy],
  },
  {
    id: "update",
    name: "workflow-toolkit",
    description:
      "检测到可用更新；更新动作保持突出，但不会改变整张卡片的来源色。",
    meta: "个人 · ~/.astro/skills",
    state: "可更新",
    stateClass: "is-outdated",
    stateKind: "outdated",
    tone: "indigo",
    Icon: RefreshCw,
    ActionIcon: RefreshCw,
    action: "更新",
    secondary: [FolderOpen, Copy],
  },
];

function SkillCard({ item }: { item: CardFixture }) {
  const { Icon, ActionIcon } = item;
  return (
    <article
      className="tool-card skill-card"
      data-skill-tone={item.tone}
      data-state={item.stateKind}
      role="listitem"
    >
      <header className="skill-card-top">
        <div className="tool-icon skill-card-icon" aria-hidden>
          <span className="tool-icon-lens" />
          <span className="tool-icon-glyph">
            <Icon size={22} strokeWidth={2} />
          </span>
        </div>
        <h3 className="skill-card-title">{item.name}</h3>
        <span
          className={
            item.stateClass
              ? `skill-card-link-badge ${item.stateClass}`
              : "skill-card-stat"
          }
        >
          {item.state}
        </span>
      </header>
      <div className="skill-card-desc">
        <p>{item.description}</p>
        <span className="skill-card-tag">{item.meta}</span>
      </div>
      <div className="skill-card-actions">
        <button
          type="button"
          className="skills-action-btn primary skill-card-primary"
        >
          <ActionIcon size={15} aria-hidden />
          <span>{item.action}</span>
        </button>
        <div
          className="skill-card-action-icons"
          role="group"
          aria-label={item.name}
        >
          {item.secondary.map((SecondaryIcon, index) => (
            <button
              key={index}
              type="button"
              className="skills-action-btn is-icon"
              aria-label={`次要操作 ${index + 1}`}
            >
              <SecondaryIcon size={15} aria-hidden />
            </button>
          ))}
        </div>
      </div>
    </article>
  );
}

function SkillCardGallery({
  list = false,
  containerWidth,
}: {
  list?: boolean;
  containerWidth?: number;
}) {
  return (
    <main
      className="skills-page"
      data-tone="indigo"
      style={{
        boxSizing: "border-box",
        minHeight: "100vh",
        padding: 28,
        background: "var(--shell-bg)",
      }}
    >
      <section
        className="skills-pane"
        style={
          containerWidth
            ? { flex: "0 0 auto", width: containerWidth }
            : undefined
        }
      >
        <header className="skills-pane-head">
          <div>
            <h2>Skill 卡片</h2>
            <p>扁平表面、单一主操作与克制的来源色</p>
          </div>
        </header>
        <div
          className={`skills-gallery ${list ? "is-list" : "is-gallery"}`}
          role="list"
        >
          {fixtures.map((item) => (
            <SkillCard key={item.id} item={item} />
          ))}
        </div>
      </section>
    </main>
  );
}

const meta = {
  title: "Pages/Plugins/SkillCards",
  component: SkillCardGallery,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof SkillCardGallery>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Gallery: Story = {};
export const List: Story = { args: { list: true } };
export const Constrained: Story = { args: { containerWidth: 540 } };
