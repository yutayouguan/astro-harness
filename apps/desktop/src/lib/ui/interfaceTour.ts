export const INTERFACE_TOUR_VERSION = 1;
export const INTERFACE_TOUR_REQUEST = "astro:interface-tour";
export type TourOutcome = "completed" | "skipped";
export type InterfaceTourState = { resolved_version: number };

export function shouldOfferInterfaceTour(state: InterfaceTourState): boolean {
  return (
    Number.isInteger(state.resolved_version) &&
    state.resolved_version >= 0 &&
    state.resolved_version < INTERFACE_TOUR_VERSION
  );
}

export function requestInterfaceTour(): void {
  window.dispatchEvent(new Event(INTERFACE_TOUR_REQUEST));
}

export const interfaceTourCopy = {
  zh: {
    welcome: "花半分钟，认识 Astro",
    intro:
      "了解任务输入、工作区、模型、右上角工具栏，以及壁纸和扩展功能在哪里。全程可跳过，不会发送消息或修改设置。",
    begin: "带我了解",
    skip: "跳过",
    dismiss: "直接开始",
    next: "下一步",
    previous: "上一步",
    done: "开始使用",
    replay: "界面导览",
    replayAction: "重新查看",
    replayDescription: "重新认识主界面，不修改已有配置。",
    saveError: "导览状态未保存，下次启动可能再次提示。",
    retry: "重试保存",
    close: "关闭提示",
    unavailable: "界面尚未就绪，请稍后重新打开导览。",
    steps: [
      [
        "composer",
        "从一个任务开始",
        "在这里描述你想做的事，也可以添加文件作为参考。导览结束后再输入，不会替你发送任何内容。",
      ],
      [
        "model",
        "选择合适的模型",
        "这里可以切换模型并调整推理选项。模型服务和密钥在设置中管理。",
      ],
      [
        "toolbar",
        "右上角的随手工具",
        "这里集中放着新建对话、项目文件、内置浏览器、终端、侧边聊天和运行摘要。按需打开对应面板；灰色按钮需要先满足相应的项目或会话条件。",
      ],
      [
        "sidebar",
        "找回任务与项目",
        "左侧可以新建对话、搜索历史，并按项目整理任务。侧栏可以折叠，也可以调整宽度。",
      ],
      [
        "workspace",
        "用工作区组织任务和文件",
        "在项目区选择或新建工作区，把本地文件夹与相关任务放在一起。选中项目后，可通过右上角的项目文件按钮查看目录，让任务围绕对应文件开展。",
      ],
      [
        "plugins",
        "扩展助手的能力",
        "在插件页管理技能与 MCP；内置工具及权限可在设置中调整。按需要启用即可。",
      ],
      [
        "appearance",
        "右下角：换壁纸，也能换配色",
        "启用壁纸时，点击小风车切换最近使用的壁纸；不足两张时会打开外观设置。没有启用壁纸时，它会在灵动配色模式下换一组颜色。更多选项在偏好设置 → 外观。",
      ],
      [
        "settings",
        "按你的习惯调整",
        "模型服务、工具权限、外观和偏好都在这里。想再看一次？点击侧栏的界面导览，或前往设置 → 关于。",
      ],
    ],
  },
  en: {
    welcome: "Meet Astro in half a minute",
    intro:
      "Find task input, workspaces, models, the upper-right toolbar, wallpapers, and extensions. Skip at any time. Nothing is sent or configured for you.",
    begin: "Show me around",
    skip: "Skip",
    dismiss: "Start directly",
    next: "Next",
    previous: "Back",
    done: "Get started",
    replay: "Interface tour",
    replayAction: "View again",
    replayDescription:
      "Explore the main interface without changing your setup.",
    saveError:
      "Tour progress was not saved. You may be prompted again next time.",
    retry: "Retry saving",
    close: "Dismiss notice",
    unavailable:
      "The interface is not ready. Please open the tour again shortly.",
    steps: [
      [
        "composer",
        "Start with a task",
        "Describe what you want to do here, or attach files for context. Start typing after the tour; no message will be sent for you.",
      ],
      [
        "model",
        "Choose a model",
        "Switch models and adjust reasoning options here. Manage providers and credentials in Settings.",
      ],
      [
        "toolbar",
        "Tools in the upper-right corner",
        "Start a conversation or open project files, the built-in browser, terminal, side chat, and run summary here. Disabled buttons require the appropriate project or conversation context.",
      ],
      [
        "sidebar",
        "Find tasks and projects",
        "Create conversations, search history, and organize tasks by project. You can collapse or resize the sidebar.",
      ],
      [
        "workspace",
        "Keep tasks and files in a workspace",
        "Select or create a workspace in Projects to group local folders and related tasks. After selecting a project, use the project-files button in the upper-right toolbar to browse its directory.",
      ],
      [
        "plugins",
        "Add capabilities",
        "Manage skills and MCP on the Plugins page. Built-in tools and permissions are in Settings. Enable what you need.",
      ],
      [
        "appearance",
        "A new wallpaper or a fresh palette",
        "With wallpapers enabled, the pinwheel cycles recent wallpapers, or opens Appearance if there are fewer than two. Without wallpaper, it reshuffles colors in dynamic-color mode. Find more options in Settings → Appearance.",
      ],
      [
        "settings",
        "Make Astro your own",
        "Configure providers, tool permissions, appearance, and preferences here. Replay this tour from the sidebar or Settings → About.",
      ],
    ],
  },
} as const;
