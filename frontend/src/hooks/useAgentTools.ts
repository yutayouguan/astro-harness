/** 内置工具目录定义（id / 图标 / 文案 key）。 */
import { useCallback, useEffect, useRef, useState } from "react";
import type { ComponentType, SVGProps } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  IconBrowser,
  IconClarify,
  IconCodeExec,
  IconConfirm,
  IconDelegate,
  IconEye,
  IconFileOps,
  IconImageGen,
  IconLocation,
  IconMemoryTool,
  IconMultiAgent,
  IconMusic,
  IconVideoGen,
  IconPresentUi,
  IconScheduled,
  IconSessionSearch,
  IconSkillsTool,
  IconTaskPlan,
  IconTerminal,
  IconTts,
  IconWebSearch,
} from "../components/ToolIcons";
import type { MessageKey } from "../i18n/messages";

const IS_TAURI =
  typeof window !== "undefined" &&
  !!(window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;

export type AgentToolId =
  | "web_search"
  | "browser"
  | "terminal"
  | "file_ops"
  | "code_exec"
  | "vision"
  | "image_gen"
  | "video_gen"
  | "tts"
  | "music"
  | "skills"
  | "memory"
  | "session_search"
  | "clarify"
  | "confirm"
  | "request_user_location"
  | "present_ui"
  | "delegate"
  | "scheduled"
  | "multi_agent"
  | "task_plan";

type IconComp = ComponentType<SVGProps<SVGSVGElement>>;

export type ToolTone =
  | "blue"
  | "green"
  | "purple"
  | "orange"
  | "pink"
  | "cyan"
  | "red"
  | "indigo"
  | "teal"
  | "amber"
  | "rose"
  | "lime"
  | "sky"
  | "emerald"
  | "fuchsia"
  | "violet";

export type ToolParam = {
  name: string;
  type: string;
  optional?: boolean;
  description?: string;
};

export type AgentToolFnDef = {
  name: string;
  description?: string;
  emoji?: string;
  params?: ToolParam[];
};

export type AgentToolDef = {
  id: AgentToolId;
  titleKey: MessageKey;
  descKey: MessageKey;
  Icon: IconComp;
  tone: ToolTone;
  params: ToolParam[];
  /** 该 toolset 下的具体工具名（来自后端 catalog） */
  tools?: string[];
  /** Lucide 图标名（catalog emoji 字段） */
  emoji?: string;
  /** API / 后端原始说明（优先于 i18n desc） */
  apiDescription?: string;
  /** toolset 下可切换的函数明细 */
  functions?: AgentToolFnDef[];
};

/** UI 元数据（图标 / i18n / 色调）；params 为后端 schema 未就绪时的回退 */
export const AGENT_TOOLS: AgentToolDef[] = [
  {
    id: "web_search",
    titleKey: "agentTools.webSearch.title",
    descKey: "agentTools.webSearch.desc",
    Icon: IconWebSearch,
    tone: "blue",
    params: [
      { name: "query", type: "string" },
      { name: "max_results", type: "number", optional: true },
    ],
  },
  {
    id: "browser",
    titleKey: "agentTools.browser.title",
    descKey: "agentTools.browser.desc",
    Icon: IconBrowser,
    tone: "cyan",
    params: [
      { name: "url", type: "string" },
    ],
  },
  {
    id: "terminal",
    titleKey: "agentTools.terminal.title",
    descKey: "agentTools.terminal.desc",
    Icon: IconTerminal,
    tone: "purple",
    params: [
      { name: "command", type: "string" },
      { name: "cwd", type: "string", optional: true },
    ],
  },
  {
    id: "file_ops",
    titleKey: "agentTools.fileOps.title",
    descKey: "agentTools.fileOps.desc",
    Icon: IconFileOps,
    tone: "green",
    params: [
      { name: "path", type: "string" },
      { name: "operation", type: "string" },
      { name: "content", type: "string", optional: true },
      { name: "offset", type: "number", optional: true },
      { name: "limit", type: "number", optional: true },
      { name: "recursive", type: "boolean", optional: true },
    ],
  },
  {
    id: "code_exec",
    titleKey: "agentTools.codeExec.title",
    descKey: "agentTools.codeExec.desc",
    Icon: IconCodeExec,
    tone: "orange",
    params: [
      { name: "code", type: "string" },
      { name: "language", type: "string", optional: true },
    ],
  },
  {
    id: "vision",
    titleKey: "agentTools.vision.title",
    descKey: "agentTools.vision.desc",
    Icon: IconEye,
    tone: "pink",
    params: [
      { name: "image_url", type: "string" },
      { name: "prompt", type: "string", optional: true },
    ],
  },
  {
    id: "image_gen",
    titleKey: "agentTools.imageGen.title",
    descKey: "agentTools.imageGen.desc",
    Icon: IconImageGen,
    tone: "indigo",
    params: [
      { name: "prompt", type: "string" },
      { name: "aspect_ratio", type: "string", optional: true },
    ],
  },
  {
    id: "video_gen",
    titleKey: "agentTools.videoGen.title",
    descKey: "agentTools.videoGen.desc",
    Icon: IconVideoGen,
    tone: "rose",
    params: [
      { name: "prompt", type: "string" },
      { name: "aspect_ratio", type: "string", optional: true },
      { name: "duration_seconds", type: "number", optional: true },
      { name: "resolution", type: "string", optional: true },
      { name: "negative_prompt", type: "string", optional: true },
      { name: "style", type: "string", optional: true },
      { name: "extend_video_id", type: "string", optional: true },
      { name: "reference_image", type: "string", optional: true },
      { name: "image", type: "string", optional: true },
      { name: "last_frame", type: "string", optional: true },
      { name: "person_generation", type: "string", optional: true },
      { name: "seed", type: "number", optional: true },
    ],
  },
  {
    id: "tts",
    titleKey: "agentTools.tts.title",
    descKey: "agentTools.tts.desc",
    Icon: IconTts,
    tone: "teal",
    params: [
      { name: "text", type: "string" },
      { name: "voice", type: "string", optional: true },
    ],
  },
  {
    id: "skills",
    titleKey: "agentTools.skills.title",
    descKey: "agentTools.skills.desc",
    Icon: IconSkillsTool,
    tone: "amber",
    params: [
      { name: "skill_id", type: "string" },
      { name: "input", type: "object", optional: true },
    ],
  },
  {
    id: "memory",
    titleKey: "agentTools.memory.title",
    descKey: "agentTools.memory.desc",
    Icon: IconMemoryTool,
    tone: "rose",
    params: [
      { name: "entry", type: "string" },
      { name: "target", type: "string", optional: true },
    ],
  },
  {
    id: "session_search",
    titleKey: "agentTools.sessionSearch.title",
    descKey: "agentTools.sessionSearch.desc",
    Icon: IconSessionSearch,
    tone: "lime",
    params: [
      { name: "query", type: "string" },
      { name: "limit", type: "number", optional: true },
    ],
  },
  {
    id: "clarify",
    titleKey: "agentTools.clarify.title",
    descKey: "agentTools.clarify.desc",
    Icon: IconClarify,
    tone: "red",
    params: [{ name: "question", type: "string" }],
  },
  {
    id: "confirm",
    titleKey: "agentTools.confirm.title",
    descKey: "agentTools.confirm.desc",
    Icon: IconConfirm,
    tone: "amber",
    params: [
      { name: "title", type: "string" },
      { name: "body", type: "string" },
    ],
  },
  {
    id: "request_user_location",
    titleKey: "agentTools.requestUserLocation.title",
    descKey: "agentTools.requestUserLocation.desc",
    Icon: IconLocation,
    tone: "teal",
    params: [{ name: "message", type: "string", optional: true }],
  },
  {
    id: "present_ui",
    titleKey: "agentTools.presentUi.title",
    descKey: "agentTools.presentUi.desc",
    Icon: IconPresentUi,
    tone: "cyan",
    params: [
      { name: "title", type: "string", optional: true },
      { name: "body", type: "string", optional: true },
      { name: "image_url", type: "string", optional: true },
      { name: "operations", type: "object", optional: true },
    ],
  },
  {
    id: "delegate",
    titleKey: "agentTools.delegate.title",
    descKey: "agentTools.delegate.desc",
    Icon: IconDelegate,
    tone: "sky",
    params: [
      { name: "goal", type: "string", optional: true },
      { name: "context", type: "string", optional: true },
      { name: "tasks", type: "object", optional: true },
      { name: "max_concurrent", type: "number", optional: true },
    ],
  },
  {
    id: "scheduled",
    titleKey: "agentTools.scheduled.title",
    descKey: "agentTools.scheduled.desc",
    Icon: IconScheduled,
    tone: "emerald",
    params: [
      { name: "cron", type: "string", optional: true },
      { name: "schedule", type: "string", optional: true },
      { name: "task", type: "string" },
    ],
  },
  {
    id: "multi_agent",
    titleKey: "agentTools.multiAgent.title",
    descKey: "agentTools.multiAgent.desc",
    Icon: IconMultiAgent,
    tone: "fuchsia",
    params: [
      { name: "goal", type: "string" },
      { name: "agents", type: "array" },
    ],
  },
  {
    id: "task_plan",
    titleKey: "agentTools.taskPlan.title",
    descKey: "agentTools.taskPlan.desc",
    Icon: IconTaskPlan,
    tone: "violet",
    params: [
      { name: "title", type: "string", optional: true },
      { name: "items", type: "array" },
    ],
  },
  {
    id: "music",
    titleKey: "agentTools.music.title",
    descKey: "agentTools.music.desc",
    Icon: IconMusic,
    tone: "pink",
    params: [
      { name: "query", type: "string", optional: true },
      { name: "action", type: "string", optional: true },
      { name: "player", type: "string", optional: true },
    ],
  },
];

type CatalogParamDto = {
  name: string;
  type: string;
  optional: boolean;
  description?: string | null;
};

type CatalogFnDto = {
  name: string;
  description: string;
  icon: string;
  params: CatalogParamDto[];
};

type CatalogItemDto = {
  id: string;
  name: string;
  description: string;
  /** Lucide 图标 id（kebab-case），来自后端 ToolEntry.icon */
  icon: string;
  params: CatalogParamDto[];
  tools: string[];
  functions?: CatalogFnDto[];
};


function defaultEnabled(): Record<AgentToolId, boolean> {
  return Object.fromEntries(
    AGENT_TOOLS.map((tool) => [tool.id, true]),
  ) as Record<AgentToolId, boolean>;
}

function mergeEnabled(
  stored: Record<string, boolean> | null | undefined,
): Record<AgentToolId, boolean> {
  const base = defaultEnabled();
  if (!stored) return base;
  for (const tool of AGENT_TOOLS) {
    if (typeof stored[tool.id] === "boolean") {
      base[tool.id] = stored[tool.id]!;
    }
  }
  return base;
}

function mapCatalogParam(p: CatalogParamDto): ToolParam {
  return {
    name: p.name,
    type: p.type,
    optional: p.optional,
    description: p.description ?? undefined,
  };
}

function mergeCatalog(catalog: CatalogItemDto[]): AgentToolDef[] {
  const byId = new Map(catalog.map((c) => [c.id, c]));
  return AGENT_TOOLS.map((tool) => {
    const item = byId.get(tool.id);
    if (!item) return tool;

    const params = item.params.map(mapCatalogParam);
    const functions: AgentToolFnDef[] =
      item.functions && item.functions.length > 0
        ? item.functions.map((fn) => ({
            name: fn.name,
            description: fn.description || undefined,
            emoji: fn.icon || undefined,
            params: fn.params.map(mapCatalogParam),
          }))
        : (item.tools ?? []).map((name) => ({
            name,
            description:
              name === item.name ? item.description || undefined : undefined,
            emoji: item.icon || undefined,
            params: name === item.name ? params : undefined,
          }));

    return {
      ...tool,
      params,
      tools: item.tools.length > 0 ? item.tools : functions.map((f) => f.name),
      emoji: item.icon || tool.emoji,
      apiDescription: item.description || tool.apiDescription,
      functions: functions.length > 0 ? functions : tool.functions,
    };
  });
}

/** 拉取后端 schemars 目录，合并到 UI 工具定义（参数以后端为准） */
export function useAgentToolDefs() {
  const [tools, setTools] = useState<AgentToolDef[]>(AGENT_TOOLS);
  const [catalogReady, setCatalogReady] = useState(false);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      if (!IS_TAURI) {
        if (!cancelled) setCatalogReady(true);
        return;
      }
      try {
        const catalog = await invoke<CatalogItemDto[]>("get_tool_catalog");
        if (!cancelled) {
          setTools(mergeCatalog(catalog));
          setCatalogReady(true);
        }
      } catch {
        if (!cancelled) setCatalogReady(true);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  return { tools, catalogReady };
}

export function useAgentTools(agentId?: string | null) {
  const [enabled, setEnabled] = useState<Record<AgentToolId, boolean>>(defaultEnabled);
  const [ready, setReady] = useState(false);
  const skipNextSave = useRef(true);
  const { tools, catalogReady } = useAgentToolDefs();

  useEffect(() => {
    let cancelled = false;
    setReady(false);
    skipNextSave.current = true;
    (async () => {
      if (!IS_TAURI) {
        try {
          const key = agentId ? `agent-tools:${agentId}` : "agent-tools";
          const raw = localStorage.getItem(key);
          if (!cancelled) {
            setEnabled(mergeEnabled(raw ? JSON.parse(raw) : null));
            setReady(true);
          }
        } catch {
          if (!cancelled) setReady(true);
        }
        return;
      }
      try {
        const stored = await invoke<Record<string, boolean>>("get_tools_enabled", {
          agentId: agentId || null,
        });
        if (!cancelled) {
          setEnabled(mergeEnabled(stored));
          setReady(true);
        }
      } catch {
        if (!cancelled) setReady(true);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [agentId]);

  useEffect(() => {
    if (!ready) return;
    if (skipNextSave.current) {
      skipNextSave.current = false;
      return;
    }
    if (!IS_TAURI) {
      try {
        const key = agentId ? `agent-tools:${agentId}` : "agent-tools";
        localStorage.setItem(key, JSON.stringify(enabled));
      } catch {
        // ignore
      }
      return;
    }
    void invoke("set_tools_enabled", { enabled, agentId: agentId || null }).catch(() => {});
  }, [enabled, ready, agentId]);

  const toggle = useCallback((id: AgentToolId) => {
    setEnabled((prev) => ({ ...prev, [id]: !prev[id] }));
  }, []);

  const setToolEnabled = useCallback((id: AgentToolId, value: boolean) => {
    setEnabled((prev) => ({ ...prev, [id]: value }));
  }, []);

  return {
    enabled,
    toggle,
    setToolEnabled,
    ready: ready && catalogReady,
    tools,
  };
}
