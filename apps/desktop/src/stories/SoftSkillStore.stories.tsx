import type { Meta, StoryObj } from "@storybook/react-vite";
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import SkillsPanel from "../components/settings/SkillsPanel";
import { ActiveAgentProvider } from "../hooks/app/useActiveAgent";
import { DialogProvider } from "../hooks/ui/DialogContext";
import { useTheme } from "../hooks/app/useTheme";
import softMeta from "./SoftMaterial.stories";
import type { StoreSkill } from "../types";

const fixtures: StoreSkill[] = [
  {
    id: "daily-report",
    name: "daily-report",
    description: "整理每日工作进展、待办与问题，输出结构清晰的日报。",
    category: "office-efficiency",
    requires_api_key: false,
  },
  {
    id: "research-notes",
    name: "research-notes",
    description: "汇总资料来源与关键论点，为研究笔记保留可追溯引用。",
    category: "knowledge-management",
    requires_api_key: true,
  },
  {
    id: "code-review",
    name: "code-review",
    description: "检查代码中的缺陷、回归风险和缺失测试。",
    category: "dev-programming",
    requires_api_key: false,
  },
  {
    id: "design-notes",
    name: "design-notes-with-a-long-name-for-layout-testing",
    description: "从需求中整理界面结构、组件状态与设计交付说明。",
    category: "design-media",
    requires_api_key: null,
  },
  {
    id: "data-report",
    name: "data-report",
    description: "整理表格数据并生成带解释的分析报告。",
    category: "data-analysis",
    requires_api_key: true,
  },
  {
    id: "broken-icon",
    name: "icon-fallback-demo",
    description: "图标加载失败时保留备用图标，名称和用途始终可见。",
    category: null,
    requires_api_key: null,
    icon_url: "data:image/png;base64,invalid",
  },
].map((item) => ({
  source: "示例来源",
  store: "skillhub",
  installs: null,
  homepage: null,
  icon_url: null,
  install_ref: `skillhub:${item.id}`,
  ...item,
}));

function StorePreview() {
  const { material, setMaterial, setMode } = useTheme();
  return (
    <div
      style={{
        height: "100%",
        display: "flex",
        flexDirection: "column",
        gap: 12,
      }}
    >
      <nav
        aria-label="样板外观"
        style={{ display: "flex", gap: 10, flexWrap: "wrap", fontSize: 12 }}
      >
        <span>样板数据 · 安装请求已拦截</span>
        <button type="button" onClick={() => setMode("light")}>
          亮色
        </button>
        <button type="button" onClick={() => setMode("dark")}>
          暗色
        </button>
        <button
          type="button"
          onClick={() => setMaterial(material === "soft" ? "glass" : "soft")}
        >
          材质：{material}
        </button>
      </nav>
      <ActiveAgentProvider>
        <DialogProvider>
          <SkillsPanel active onInstallWithAgent={() => {}} />
        </DialogProvider>
      </ActiveAgentProvider>
    </div>
  );
}

const meta = {
  title: "Design/Soft Skill Store",
  component: StorePreview,
  decorators: softMeta.decorators,
  beforeEach: () => {
    mockIPC((command, payload) => {
      if (command === "get_config")
        return {
          active_agent_id: "default",
          agents: [],
          workspace_dir: "/tmp/astro-skill-preview",
        };
      if (command === "list_installed_skills")
        return [
          {
            id: "daily-report",
            name: "daily-report",
            description: fixtures[0].description,
            path: "/tmp/astro-skill-preview/daily-report",
            source_dir: "/tmp/astro-skill-preview",
            enabled: true,
          },
        ];
      if (
        command === "list_skill_origins" ||
        command === "list_skill_backups" ||
        command === "check_skill_updates"
      )
        return [];
      if (command === "get_agent_usage_stats") return { skills: {} };
      if (command === "search_store_skills") {
        const { query, category, apiKey, page } = payload as {
          query: string;
          category: string | null;
          apiKey: string | null;
          page: number;
        };
        if (page > 1) return [];
        return fixtures.filter(
          (skill) =>
            (!query || `${skill.name} ${skill.description}`.includes(query)) &&
            (!category || skill.category === category) &&
            (!apiKey ||
              (apiKey === "required"
                ? skill.requires_api_key === true
                : skill.requires_api_key === false)),
        );
      }
      if (command === "set_app_menu_locale" || command.startsWith("plugin:"))
        return null;
      throw new Error(`Skill preview blocked command: ${command}`);
    });
    return () => clearMocks();
  },
} satisfies Meta<typeof StorePreview>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Gallery: Story = {};
