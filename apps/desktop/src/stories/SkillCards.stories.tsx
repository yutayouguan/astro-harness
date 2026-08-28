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
    description: "Browser automation for agents that need to inspect and interact with websites.",
    meta: "个人 · ~/.astro/skills",
    state: "已启用",
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
    tone: "amber",
    Icon: HardDrive,
    ActionIcon: Link2,
    action: "链接",
    secondary: [Eye, FolderOpen, Copy],
  },
  {
    id: "market",
    name: "research-assistant",
    description: "Research workflow with source collection, synthesis, and structured reports.",
    meta: "SkillHub",
    state: "1.2k 安装",
    tone: "purple",
    Icon: CloudDownload,
    ActionIcon: CloudDownload,
    action: "安装",
    secondary: [Bot, Copy],
  },
];

function SkillCard({ item }: { item: CardFixture }) {
  const { Icon, ActionIcon } = item;
  return (
    <article className="tool-card skill-card" data-skill-tone={item.tone} role="listitem">
      <header className="skill-card-top">
        <div className="tool-icon skill-card-icon" aria-hidden>
          <span className="tool-icon-lens" />
          <span className="tool-icon-glyph"><Icon size={22} strokeWidth={2} /></span>
        </div>
        <h3 className="skill-card-title">{item.name}</h3>
        <span className="skill-card-stat">{item.state}</span>
      </header>
      <div className="skill-card-desc">
        <p>{item.description}</p>
        <span className="skill-card-tag">{item.meta}</span>
      </div>
      <div className="skill-card-actions">
        <button type="button" className="skills-action-btn primary skill-card-primary">
          <ActionIcon size={15} aria-hidden />
          <span>{item.action}</span>
        </button>
        <div className="skill-card-action-icons">
          {item.secondary.map((SecondaryIcon, index) => (
            <button key={index} type="button" className="skills-action-btn is-icon" aria-label={`次要操作 ${index + 1}`}>
              <SecondaryIcon size={15} aria-hidden />
            </button>
          ))}
        </div>
      </div>
    </article>
  );
}

function SkillCardGallery({ list = false }: { list?: boolean }) {
  return (
    <main className="skills-page" data-tone="indigo" style={{ boxSizing: "border-box", minHeight: "100vh", padding: 28, background: "var(--shell-bg)" }}>
      <section className="skills-pane">
        <header className="skills-pane-head">
          <div>
            <h2>Skill 卡片</h2>
            <p>扁平表面、单一主操作与克制的来源色</p>
          </div>
        </header>
        <div className={`skills-gallery ${list ? "is-list" : "is-gallery"}`} role="list">
          {fixtures.map((item) => <SkillCard key={item.id} item={item} />)}
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
