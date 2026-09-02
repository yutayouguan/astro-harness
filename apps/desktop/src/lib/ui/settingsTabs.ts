/** 设置页 tab 元数据：侧栏导航与标题栏共用同一份定义。 */
import type { ComponentType, SVGProps } from "react";
import {
  IconAtom,
  IconBrowser,
  IconChat,
  IconContext,
  IconDiagnostics,
  IconInsights,
  IconMemory,
  IconModelMarket,
  IconProviders,
  IconSettings,
  IconSparkles,
  IconTerminal,
  IconTools,
} from "../../components/icons";
import type { SettingsTabId } from "./navConfig";

export type SettingsTabMeta = {
  id: SettingsTabId;
  label: string;
  Icon: ComponentType<SVGProps<SVGSVGElement>>;
};

export type SettingsTabGroup = {
  id: "basics" | "intelligence" | "extensions" | "system";
  label: string;
  items: SettingsTabMeta[];
};

/** 设置侧栏按任务语义分组，避免所有入口看起来拥有相同权重。 */
export const SETTINGS_TAB_GROUPS: SettingsTabGroup[] = [
  {
    id: "basics",
    label: "基础",
    items: [
      { id: "preferences", label: "通用", Icon: IconSettings },
      { id: "preferences:appearance", label: "外观", Icon: IconSparkles },
      { id: "preferences:conversation", label: "对话", Icon: IconChat },
      { id: "terminal", label: "终端", Icon: IconTerminal },
    ],
  },
  {
    id: "intelligence",
    label: "智能",
    items: [
      { id: "preferences:context", label: "上下文与压缩", Icon: IconContext },
      { id: "providers", label: "模型配置", Icon: IconProviders },
      { id: "tools", label: "工具", Icon: IconTools },
      { id: "memory", label: "记忆", Icon: IconMemory },
    ],
  },
  {
    id: "extensions",
    label: "扩展",
    items: [
      { id: "browser", label: "浏览器", Icon: IconBrowser },
      { id: "models", label: "模型市场", Icon: IconModelMarket },
      { id: "insights", label: "洞察", Icon: IconInsights },
    ],
  },
  {
    id: "system",
    label: "系统",
    items: [
      { id: "preferences:diagnostics", label: "诊断", Icon: IconDiagnostics },
      { id: "preferences:about", label: "关于", Icon: IconAtom },
    ],
  },
];

/** 标题栏仍消费一份扁平列表，并与侧栏分组保持同源。 */
export const SETTINGS_TABS: SettingsTabMeta[] = SETTINGS_TAB_GROUPS.flatMap(
  (group) => group.items,
);

/** 不在侧栏平铺，只能由跳转进入的 tab */
const EXTRA_SETTINGS_TABS: SettingsTabMeta[] = [
  { id: "evolution", label: "自进化", Icon: IconSparkles },
];

export function settingsTabMeta(tab: SettingsTabId): SettingsTabMeta {
  return (
    SETTINGS_TABS.find((item) => item.id === tab) ??
    EXTRA_SETTINGS_TABS.find((item) => item.id === tab) ??
    SETTINGS_TABS[0]
  );
}
