/** 插件能力中心入口：Skills 与 MCP 使用各自的来源分类导航。 */
import SkillsPanel, { type SkillsPanelProps } from "../settings/SkillsPanel";

export default function PluginsPage(props: SkillsPanelProps) {
  return <SkillsPanel {...props} />;
}
