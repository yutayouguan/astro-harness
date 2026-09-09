type StarterCopy = { title: string; description: string; prompt: string };
type StarterExample = { id: string; zh: StarterCopy; en: StarterCopy };

/** Ready-to-edit drafts only. Choosing an example never executes the task. */
export const STARTER_TASKS = [
  {
    id: "desktop",
    zh: {
      title: "看看我的桌面",
      description: "从桌面文件出发，发现至少 5 个协作方向",
      prompt:
        "请看看我桌面的文件，结合文件类型和项目线索，给出至少 5 个以后我们可以协作的方向。先确认实际桌面路径，只查看顶层文件名和类型，不批量读取文件内容；需要深入查看时先问我。每个方向说明可以帮我做什么、需要我提供什么，以及第一步怎么开始。不要移动、删除或修改任何文件。",
    },
    en: {
      title: "Explore my desktop",
      description: "Find at least 5 ways we could work together",
      prompt:
        "Look at the files on my desktop and suggest at least 5 ways we could collaborate, based on file types and project clues. Resolve the actual desktop path first and inspect only top-level names and types; ask before reading file contents. For each direction, explain how you can help, what you need from me, and a concrete first step. Do not move, delete, or modify files.",
    },
  },
  {
    id: "organize",
    zh: {
      title: "规划文件整理",
      description: "先看目录，再给出分类和命名方案",
      prompt:
        "帮我规划一次文件整理。先问我想整理哪个目录，再只读查看文件名和类型，给出目录结构、分类规则、命名建议和需要我确认的疑似重复文件清单。先展示方案，不要移动、重命名或删除文件。",
    },
    en: {
      title: "Plan a file tidy-up",
      description: "Propose folders, categories, and naming rules",
      prompt:
        "Help me plan a file tidy-up. Ask which folder I want to organize, then inspect file names and types read-only. Propose a folder structure, categories, naming rules, and possible duplicates for me to review. Present the plan first; do not move, rename, or delete anything.",
    },
  },
  {
    id: "documents",
    zh: {
      title: "读懂一份文档",
      description: "提炼重点、待办和需要澄清的问题",
      prompt:
        "帮我快速读懂一份文档。先请我选择文件或提供路径；读取我确认的文档后，用一句话概括主题，列出关键结论、带出处的依据、待办事项和需要澄清的问题。不确定的地方请明确标注，不要编造内容或修改原文件。",
    },
    en: {
      title: "Understand a document",
      description: "Extract key points, actions, and open questions",
      prompt:
        "Help me understand a document. Ask me to select a file or provide its path. After reading the document I confirm, summarize its topic in one sentence, then list key conclusions, supporting references, action items, and open questions. Flag uncertainty; do not invent content or modify the original.",
    },
  },
  {
    id: "weekly",
    zh: {
      title: "起草一份周报",
      description: "把工作记录变成成果、风险和下周计划",
      prompt:
        "帮我起草本周工作周报。先问我时间范围和希望使用的工作记录或文件；只根据我提供或确认的材料，整理本周成果、进行中的事项、风险与阻碍、下周计划。缺少的信息列为待补充，不要虚构进度，也不要代我发送。",
    },
    en: {
      title: "Draft a weekly update",
      description: "Turn work notes into progress and next steps",
      prompt:
        "Help me draft a weekly work update. First ask for the date range and the notes or files to use. Based only on materials I provide or confirm, organize accomplishments, work in progress, risks and blockers, and next week's plan. Mark missing information instead of inventing progress. Do not send the update on my behalf.",
    },
  },
  {
    id: "project",
    zh: {
      title: "熟悉一个项目",
      description: "梳理结构、启动方法和优先处理的任务",
      prompt:
        "帮我熟悉一个项目。先确认项目目录，再只读查看说明文档和主要目录结构，梳理项目目标、关键模块、文档中记录的启动和测试方法，以及 3 个值得优先处理的任务。区分已确认的信息和推测；不要安装依赖、运行脚本或修改代码。",
    },
    en: {
      title: "Get to know a project",
      description: "Map its structure, setup, and priority tasks",
      prompt:
        "Help me get to know a project. Confirm its directory, then read its documentation and main directory structure without making changes. Explain its purpose, key modules, documented setup and test commands, and 3 useful priority tasks. Separate verified facts from assumptions. Do not install dependencies, run scripts, or modify code.",
    },
  },
  {
    id: "automation",
    zh: {
      title: "减少重复工作",
      description: "找出适合自动化的流程，从小任务开始",
      prompt:
        "帮我找出日常工作中值得自动化的事情。先问我 3 个简短问题，了解重复任务、频率和使用的工具，再提出 3 个具体方案，说明预计节省的时间、所需权限和风险，推荐一个最小可行的起点。先讨论方案，不要创建定时任务、连接账号或修改系统设置。",
    },
    en: {
      title: "Reduce repetitive work",
      description: "Find useful automations and start small",
      prompt:
        "Help me find worthwhile automations in my daily work. Ask 3 short questions about repetitive tasks, frequency, and tools, then propose 3 concrete options with estimated time savings, required permissions, and risks. Recommend a small first step. Discuss the plan first; do not create schedules, connect accounts, or change system settings.",
    },
  },
] as const satisfies readonly StarterExample[];
