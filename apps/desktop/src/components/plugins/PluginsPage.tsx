/** 插件能力中心入口：Skills 与 MCP 共用同一套作用域导航。 */
import SkillsPanel, {
  type SkillsPanelProps,
} from "../settings/SkillsPanel";

export default function PluginsPage(props: SkillsPanelProps) {
  return <SkillsPanel {...props} />;
}
