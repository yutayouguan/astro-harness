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
      "了解从哪里开始任务，以及模型、历史和扩展功能在哪里。全程可跳过，不会发送消息或修改设置。",
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
        "sidebar",
        "找回任务与项目",
        "左侧可以新建对话、搜索历史，并按项目整理任务。侧栏可以折叠，也可以调整宽度。",
      ],
      [
        "plugins",
        "扩展助手的能力",
        "在插件页管理技能与 MCP；内置工具及权限可在设置中调整。按需要启用即可。",
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
      "Find where to start a task, choose models, revisit history, and add capabilities. Skip at any time. Nothing is sent or configured for you.",
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
        "sidebar",
        "Find tasks and projects",
        "Create conversations, search history, and organize tasks by project. You can collapse or resize the sidebar.",
      ],
      [
        "plugins",
        "Add capabilities",
        "Manage skills and MCP on the Plugins page. Built-in tools and permissions are in Settings. Enable what you need.",
      ],
      [
        "settings",
        "Make Astro your own",
        "Configure providers, tool permissions, appearance, and preferences here. Replay this tour from the sidebar or Settings → About.",
      ],
    ],
  },
} as const;
