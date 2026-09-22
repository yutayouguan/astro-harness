import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { Meta, StoryObj } from "@storybook/react-vite";
import {
  ComposerPalette,
  type PaletteItem,
  type PaletteKind,
} from "../components/chat/ComposerPalette";
import { LocaleProvider } from "../i18n/LocaleContext";

const mentionItems: PaletteItem[] = [
  {
    id: "mention-agent-astro",
    title: "@Astro",
    description: "切换到该 Agent（写入活跃 Agent）",
    icon: "◎",
    group: "Agents",
    mentionKind: "agent",
    action: "insert",
  },
  {
    id: "mention-skill-aihot",
    title: "@aihot",
    description: "查询 AIHOT 的中文 AI 资讯、精选、当前热点和日报。",
    icon: "✦",
    group: "Skills",
    mentionKind: "skill",
    action: "insert",
  },
  {
    id: "mention-skill-desktop-pet-creator",
    title: "@desktop-pet-creator",
    description:
      "从用户照片生成 Astro 桌宠，可选配套壁纸、保存和切换宠物场景。",
    icon: "✦",
    group: "Skills",
    mentionKind: "skill",
    action: "insert",
  },
  {
    id: "mention-mcp-docs",
    title: "@docs",
    description: "启用该 MCP 服务",
    icon: "⬡",
    group: "MCP",
    mentionKind: "mcp",
    action: "insert",
  },
];

const slashItems: PaletteItem[] = [
  {
    id: "slash-new",
    title: "/new",
    description: "新建会话",
    icon: "✧",
    group: "指令",
    action: "new_chat",
  },
  {
    id: "slash-skill-aihot",
    title: "/aihot",
    description: "加载该技能全文到本轮（对齐 Hermes）",
    icon: "✦",
    group: "技能",
    action: "insert_skill",
  },
];

function PaletteSample({
  kind,
  items,
  activeIndex,
  selectedId,
}: {
  kind: PaletteKind;
  items: PaletteItem[];
  activeIndex: number;
  selectedId?: string;
}) {
  return (
    <main
      style={{
        boxSizing: "border-box",
        width: "100vw",
        minHeight: "100vh",
        padding: "32px 24px",
        background: "var(--shell-bg)",
        color: "var(--ink)",
      }}
    >
      <div style={{ maxWidth: 720, margin: "0 auto" }}>
        <ComposerPalette
          kind={kind}
          items={items}
          query=""
          activeIndex={activeIndex}
          selectedId={selectedId}
          onHover={() => {}}
          onSelect={() => {}}
          onClose={() => {}}
        />
      </div>
    </main>
  );
}

const meta = {
  title: "Chat/Composer Palette",
  component: PaletteSample,
  beforeEach: () => {
    mockIPC(() => null);
    return () => clearMocks();
  },
  decorators: [
    (Story) => (
      <LocaleProvider>
        <Story />
      </LocaleProvider>
    ),
  ],
  parameters: { controls: { disable: true }, layout: "fullscreen" },
} satisfies Meta<typeof PaletteSample>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Mention: Story = {
  args: { kind: "mention", items: mentionItems, activeIndex: 1 },
};

/** 同一材质下的 `/` 面板：分组标签与行式保持一致。 */
export const Slash: Story = {
  args: {
    kind: "slash",
    items: slashItems,
    activeIndex: 0,
    selectedId: "slash-new",
  },
};
