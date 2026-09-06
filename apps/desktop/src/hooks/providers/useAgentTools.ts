/** 内置工具目录定义（id / 图标 / 文案 key）。 */
import { useCallback, useEffect, useRef, useState } from "react";
import type { ComponentType, SVGProps } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  IconAudioUnderstand,
  IconClarify,
  IconCodeExec,
  IconEye,
  IconImageGen,
  IconMemoryTool,
  IconMultiAgent,
  IconMusic,
  IconRobotics,
  IconVideoGen,
  IconPresentUi,
  IconScheduled,
  IconSessionSearch,
  IconSkillsTool,
  IconTodo,
  IconTerminal,
  IconTts,
  IconWebSearch,
} from "../../components/icons/ToolIcons";
import type { MessageKey } from "../../i18n/messages";
import {
  mergeToolCatalog,
  type CatalogItemDto,
} from "../../lib/tools/agentToolCatalog";

const IS_TAURI =
  typeof window !== "undefined" &&
  !!(window as unknown as { __TAURI_INTERNALS__?: unknown })
    .__TAURI_INTERNALS__;

export type AgentToolId =
  | "web_search"
  | "browser"
  | "exec_command"
  | "apply_patch"
  | "code_exec"
  | "image_analyze"
  | "robotics"
  | "audio_analyze"
  | "image_gen"
  | "ui_style"
  | "video_gen"
  | "video_analyze"
  | "speech_gen"
  | "music_gen"
  | "skills"
  | "memory"
  | "context_search"
  | "pin_context"
  | "ask_user"
  | "request_user_input_async"
  | "switch_mode"
  | "present"
  | "subagents"
  | "cron"
  | "workflow"
  | "persona"
  | "todo";

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
  /** 模型可见调用名，namespace 工具为 namespace.child。 */
  name: string;
  namespace?: string;
  registeredName?: string;
  description?: string;
  emoji?: string;
  params?: ToolParam[];
  exposure?: string;
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
  namespace?: string;
  registeredName?: string;
  exposure?: string;
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
      { name: "url", type: "string", optional: true },
      { name: "urls", type: "string", optional: true },
      { name: "max_chars", type: "number", optional: true },
    ],
  },
  {
    id: "browser",
    titleKey: "agentTools.browser.title",
    descKey: "agentTools.browser.desc",
    Icon: IconWebSearch,
    tone: "cyan",
    params: [
      { name: "url", type: "string", optional: true },
      { name: "selector", type: "string", optional: true },
      { name: "text", type: "string", optional: true },
      { name: "intent", type: "string", optional: true },
    ],
  },
  {
    id: "exec_command",
    titleKey: "agentTools.execCommand.title",
    descKey: "agentTools.execCommand.desc",
    Icon: IconTerminal,
    tone: "purple",
    params: [
      { name: "action", type: "string", optional: true },
      { name: "command", type: "string", optional: true },
      { name: "cwd", type: "string", optional: true },
      { name: "timeout_secs", type: "number", optional: true },
      { name: "background", type: "boolean", optional: true },
      { name: "id", type: "string", optional: true },
      { name: "offset", type: "number", optional: true },
    ],
  },
  {
    id: "apply_patch",
    titleKey: "agentTools.applyPatch.title",
    descKey: "agentTools.applyPatch.desc",
    Icon: IconTerminal,
    tone: "green",
    params: [{ name: "patch", type: "string" }],
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
    id: "image_analyze",
    titleKey: "agentTools.imageAnalyze.title",
    descKey: "agentTools.imageAnalyze.desc",
    Icon: IconEye,
    tone: "pink",
    params: [
      { name: "image_url", type: "string" },
      { name: "prompt", type: "string", optional: true },
    ],
  },
  {
    id: "robotics",
    titleKey: "agentTools.robotics.title",
    descKey: "agentTools.robotics.desc",
    Icon: IconRobotics,
    tone: "pink",
    params: [
      { name: "image_url", type: "string" },
      { name: "mode", type: "string", optional: true },
      { name: "prompt", type: "string", optional: true },
      { name: "queries", type: "string", optional: true },
      { name: "robot_api", type: "string", optional: true },
    ],
  },
  {
    id: "audio_analyze",
    titleKey: "agentTools.audioAnalyze.title",
    descKey: "agentTools.audioAnalyze.desc",
    Icon: IconAudioUnderstand,
    tone: "cyan",
    params: [
      { name: "audio_url", type: "string" },
      { name: "prompt", type: "string", optional: true },
      { name: "mode", type: "string", optional: true },
      { name: "start", type: "string", optional: true },
      { name: "end", type: "string", optional: true },
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
    id: "ui_style",
    titleKey: "agentTools.uiStyle.title",
    descKey: "agentTools.uiStyle.desc",
    Icon: IconImageGen,
    tone: "cyan",
    params: [
      { name: "action", type: "string", optional: true },
      { name: "name", type: "string", optional: true },
      { name: "id", type: "string", optional: true },
      { name: "wallpaperPath", type: "string", optional: true },
      { name: "fit", type: "string", optional: true },
      { name: "shade", type: "number", optional: true },
      { name: "blur", type: "number", optional: true },
      { name: "adaptiveColor", type: "boolean", optional: true },
      { name: "lightTokens", type: "object", optional: true },
      { name: "darkTokens", type: "object", optional: true },
      { name: "iconMotion", type: "string", optional: true },
      { name: "iconStrokeWidth", type: "number", optional: true },
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
    id: "video_analyze",
    titleKey: "agentTools.videoAnalyze.title",
    descKey: "agentTools.videoAnalyze.desc",
    Icon: IconVideoGen,
    tone: "rose",
    params: [
      { name: "video_url", type: "string" },
      { name: "prompt", type: "string", optional: true },
      { name: "mode", type: "string", optional: true },
    ],
  },
  {
    id: "speech_gen",
    titleKey: "agentTools.speechGen.title",
    descKey: "agentTools.speechGen.desc",
    Icon: IconTts,
    tone: "teal",
    params: [
      { name: "text", type: "string" },
      { name: "voice", type: "string", optional: true },
    ],
  },
  {
    id: "music_gen",
    titleKey: "agentTools.musicGen.title",
    descKey: "agentTools.musicGen.desc",
    Icon: IconMusic,
    tone: "violet",
    params: [
      { name: "prompt", type: "string" },
      { name: "model", type: "string", optional: true },
      { name: "reference_images", type: "string", optional: true },
      { name: "format", type: "string", optional: true },
    ],
  },
  {
    id: "skills",
    titleKey: "agentTools.skills.title",
    descKey: "agentTools.skills.desc",
    Icon: IconSkillsTool,
    tone: "amber",
    params: [
      { name: "action", type: "string", optional: true },
      { name: "skill_id", type: "string", optional: true },
      { name: "manage_action", type: "string", optional: true },
      { name: "content", type: "string", optional: true },
      { name: "description", type: "string", optional: true },
      { name: "old_string", type: "string", optional: true },
      { name: "new_string", type: "string", optional: true },
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
    id: "context_search",
    titleKey: "agentTools.contextSearch.title",
    descKey: "agentTools.contextSearch.desc",
    Icon: IconSessionSearch,
    tone: "lime",
    params: [
      { name: "query", type: "string" },
      { name: "scope", type: "string", optional: true },
      { name: "limit", type: "number", optional: true },
    ],
  },
  {
    id: "pin_context",
    titleKey: "agentTools.pinContext.title",
    descKey: "agentTools.pinContext.desc",
    Icon: IconSessionSearch,
    tone: "lime",
    params: [
      { name: "action", type: "string" },
      { name: "content", type: "string", optional: true },
      { name: "id", type: "string", optional: true },
      { name: "title", type: "string", optional: true },
    ],
  },
  {
    id: "ask_user",
    titleKey: "agentTools.askUser.title",
    descKey: "agentTools.askUser.desc",
    Icon: IconClarify,
    tone: "red",
    params: [
      { name: "mode", type: "string", optional: true },
      { name: "questions", type: "array", optional: true },
      { name: "title", type: "string", optional: true },
      { name: "body", type: "string", optional: true },
      { name: "message", type: "string", optional: true },
    ],
  },
  {
    id: "request_user_input_async",
    titleKey: "agentTools.sendUserMessageAsync.title",
    descKey: "agentTools.sendUserMessageAsync.desc",
    Icon: IconClarify,
    tone: "blue",
    params: [{ name: "questions", type: "array" }],
  },
  {
    id: "switch_mode",
    titleKey: "agentTools.switchMode.title",
    descKey: "agentTools.switchMode.desc",
    Icon: IconClarify,
    tone: "indigo",
    params: [
      { name: "to", type: "string" },
      { name: "reason", type: "string" },
      { name: "summary", type: "string", optional: true },
    ],
  },
  {
    id: "present",
    titleKey: "agentTools.present.title",
    descKey: "agentTools.present.desc",
    Icon: IconPresentUi,
    tone: "cyan",
    params: [
      { name: "kind", type: "string", optional: true },
      { name: "title", type: "string", optional: true },
      { name: "body", type: "string", optional: true },
      { name: "image_url", type: "string", optional: true },
      { name: "operations", type: "object", optional: true },
      { name: "variant", type: "string", optional: true },
      { name: "status", type: "string", optional: true },
      { name: "metrics", type: "array", optional: true },
    ],
  },
  {
    id: "subagents",
    titleKey: "agentTools.subagents.title",
    descKey: "agentTools.subagents.desc",
    Icon: IconMultiAgent,
    tone: "sky",
    params: [],
    tools: [
      "spawn_agent",
      "list_agents",
      "send_message",
      "followup_task",
      "wait_agent",
      "interrupt_agent",
    ],
    functions: [
      {
        name: "spawn_agent",
        params: [
          { name: "task_name", type: "string" },
          { name: "message", type: "string" },
          { name: "agent_type", type: "string", optional: true },
          { name: "model", type: "string", optional: true },
          { name: "reasoning_effort", type: "string", optional: true },
          { name: "fork_turns", type: "string", optional: true },
        ],
      },
      {
        name: "list_agents",
        params: [{ name: "path_prefix", type: "string", optional: true }],
      },
      {
        name: "send_message",
        params: [
          { name: "target", type: "string" },
          { name: "message", type: "string" },
        ],
      },
      {
        name: "followup_task",
        params: [
          { name: "target", type: "string" },
          { name: "message", type: "string" },
        ],
      },
      {
        name: "wait_agent",
        params: [{ name: "timeout_ms", type: "number", optional: true }],
      },
      {
        name: "interrupt_agent",
        params: [{ name: "target", type: "string" }],
      },
    ],
  },
  {
    id: "cron",
    titleKey: "agentTools.cron.title",
    descKey: "agentTools.cron.desc",
    Icon: IconScheduled,
    tone: "emerald",
    params: [
      { name: "action", type: "string" },
      { name: "schedule", type: "string", optional: true },
      { name: "cron", type: "string", optional: true },
      { name: "task", type: "string", optional: true },
      { name: "id", type: "string", optional: true },
    ],
  },
  {
    id: "workflow",
    titleKey: "agentTools.workflow.title",
    descKey: "agentTools.workflow.desc",
    Icon: IconScheduled,
    tone: "fuchsia",
    params: [],
  },
  {
    id: "persona",
    titleKey: "agentTools.persona.title",
    descKey: "agentTools.persona.desc",
    Icon: IconMultiAgent,
    tone: "indigo",
    params: [
      { name: "name", type: "string" },
      { name: "id", type: "string", optional: true },
      { name: "activate", type: "boolean", optional: true },
      { name: "inherit_config", type: "boolean", optional: true },
      { name: "profile", type: "object", optional: true },
    ],
  },
  {
    id: "todo",
    titleKey: "agentTools.todo.title",
    descKey: "agentTools.todo.desc",
    Icon: IconTodo,
    tone: "violet",
    params: [
      { name: "action", type: "string", optional: true },
      { name: "title", type: "string", optional: true },
      { name: "items", type: "array" },
      { name: "plan_id", type: "string", optional: true },
    ],
  },
];

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

export function mergeCatalog(catalog: CatalogItemDto[]): AgentToolDef[] {
  return mergeToolCatalog(AGENT_TOOLS, catalog);
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
  const [enabled, setEnabled] =
    useState<Record<AgentToolId, boolean>>(defaultEnabled);
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
        const stored = await invoke<Record<string, boolean>>(
          "get_tools_enabled",
          {
            agentId: agentId || null,
          },
        );
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
    void invoke("set_tools_enabled", {
      enabled,
      agentId: agentId || null,
    }).catch(() => {});
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
