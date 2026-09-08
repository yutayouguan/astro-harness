/** 设置页 tab 元数据：侧栏导航与标题栏共用同一份定义。 */
import type { ComponentType, SVGProps } from "react";
import {
  IconAbout,
  IconAppearance,
  IconBrowser,
  IconChat,
  IconContext,
  IconDependencies,
  IconDiagnostics,
  IconInsights,
  IconMemory,
  IconModelMarket,
  IconPet,
  IconProviders,
  IconSettings,
  IconSparkles,
  IconTerminal,
  IconTools,
} from "../../components/icons";
import type { MessageKey } from "../../i18n/messages";
import type { SettingsTabId } from "./navConfig";

export type SettingsTabMeta = {
  id: SettingsTabId;
  labelKey: MessageKey;
  Icon: ComponentType<SVGProps<SVGSVGElement>>;
};

export type SettingsTabGroup = {
  id: "basics" | "intelligence" | "extensions" | "system";
  labelKey: MessageKey;
  items: SettingsTabMeta[];
};

/** 设置侧栏按任务语义分组，避免所有入口看起来拥有相同权重。 */
export const SETTINGS_TAB_GROUPS: SettingsTabGroup[] = [
  {
    id: "basics",
    labelKey: "settings.sidebar.group.basics",
    items: [
      {
        id: "preferences",
        labelKey: "settings.sidebar.tab.general",
        Icon: IconSettings,
      },
      {
        id: "preferences:appearance",
        labelKey: "settings.sidebar.tab.appearance",
        Icon: IconAppearance,
      },
      {
        id: "preferences:conversation",
        labelKey: "settings.sidebar.tab.conversation",
        Icon: IconChat,
      },
      {
        id: "desktop-pet",
        labelKey: "settings.sidebar.tab.desktopPet",
        Icon: IconPet,
      },
      {
        id: "terminal",
        labelKey: "settings.sidebar.tab.terminal",
        Icon: IconTerminal,
      },
    ],
  },
  {
    id: "intelligence",
    labelKey: "settings.sidebar.group.intelligence",
    items: [
      {
        id: "preferences:context",
        labelKey: "settings.sidebar.tab.context",
        Icon: IconContext,
      },
      {
        id: "providers",
        labelKey: "settings.sidebar.tab.providers",
        Icon: IconProviders,
      },
      {
        id: "tools",
        labelKey: "settings.sidebar.tab.tools",
        Icon: IconTools,
      },
      {
        id: "memory",
        labelKey: "settings.sidebar.tab.memory",
        Icon: IconMemory,
      },
    ],
  },
  {
    id: "extensions",
    labelKey: "settings.sidebar.group.extensions",
    items: [
      {
        id: "browser",
        labelKey: "settings.sidebar.tab.browser",
        Icon: IconBrowser,
      },
      {
        id: "models",
        labelKey: "settings.sidebar.tab.models",
        Icon: IconModelMarket,
      },
      {
        id: "insights",
        labelKey: "settings.sidebar.tab.insights",
        Icon: IconInsights,
      },
    ],
  },
  {
    id: "system",
    labelKey: "settings.sidebar.group.system",
    items: [
      {
        id: "environment-dependencies",
        labelKey: "settings.sidebar.tab.environmentDependencies",
        Icon: IconDependencies,
      },
      {
        id: "preferences:diagnostics",
        labelKey: "settings.sidebar.tab.diagnostics",
        Icon: IconDiagnostics,
      },
      {
        id: "preferences:about",
        labelKey: "settings.sidebar.tab.about",
        Icon: IconAbout,
      },
    ],
  },
];

/** 标题栏仍消费一份扁平列表，并与侧栏分组保持同源。 */
export const SETTINGS_TABS: SettingsTabMeta[] = SETTINGS_TAB_GROUPS.flatMap(
  (group) => group.items,
);

/** 不在侧栏平铺，只能由跳转进入的 tab */
const EXTRA_SETTINGS_TABS: SettingsTabMeta[] = [
  {
    id: "evolution",
    labelKey: "settings.sidebar.tab.evolution",
    Icon: IconSparkles,
  },
];

export function settingsTabMeta(tab: SettingsTabId): SettingsTabMeta {
  return (
    SETTINGS_TABS.find((item) => item.id === tab) ??
    EXTRA_SETTINGS_TABS.find((item) => item.id === tab) ??
    SETTINGS_TABS[0]
  );
}
