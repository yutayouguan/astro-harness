import type { TerminalExecutionMode } from "./terminalSettings";

export const MAX_TERMINAL_TABS = 8;
const STORAGE_PREFIX = "astro.terminal.tabs.v1.";

export type TerminalTab = {
  clientId: string;
  title: string;
  cwd: string;
  executionMode: TerminalExecutionMode;
  agentDefault: boolean;
};

type StoredLayout = {
  activeClientId?: string;
  tabs?: unknown[];
};

export type TerminalTabLayout = {
  activeClientId: string;
  tabs: TerminalTab[];
};

export function createTerminalClientId(): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID();
  }
  return `terminal-${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

export function defaultTerminalTabs(
  projectRoot: string,
  userTitle: string,
  agentTitle: string,
  userExecutionMode: TerminalExecutionMode = "system",
): TerminalTabLayout {
  const user = {
    clientId: createTerminalClientId(),
    title: userTitle,
    cwd: projectRoot,
    executionMode: userExecutionMode,
    agentDefault: false,
  };
  const agent = {
    clientId: createTerminalClientId(),
    title: agentTitle,
    cwd: projectRoot,
    executionMode: "project" as const,
    agentDefault: true,
  };
  return { tabs: [user, agent], activeClientId: user.clientId };
}

function normalizeTab(value: unknown, projectRoot: string): TerminalTab | null {
  if (!value || typeof value !== "object") return null;
  const input = value as Record<string, unknown>;
  const clientId = typeof input.clientId === "string" ? input.clientId.trim() : "";
  if (!clientId || clientId.length > 128) return null;
  const title = typeof input.title === "string" ? input.title.trim().slice(0, 60) : "";
  return {
    clientId,
    title: title || "Terminal",
    cwd: projectRoot,
    executionMode: input.executionMode === "project" ? "project" : "system",
    agentDefault: input.agentDefault === true && input.executionMode === "project",
  };
}

export function readTerminalTabLayout(
  projectId: string,
  projectRoot: string,
  userTitle: string,
  agentTitle: string,
  userExecutionMode: TerminalExecutionMode = "system",
): TerminalTabLayout {
  const fallback = () =>
    defaultTerminalTabs(projectRoot, userTitle, agentTitle, userExecutionMode);
  if (typeof window === "undefined") return fallback();
  try {
    const raw = window.localStorage.getItem(`${STORAGE_PREFIX}${projectId}`);
    if (!raw) return fallback();
    const parsed = JSON.parse(raw) as StoredLayout;
    const seen = new Set<string>();
    const tabs = (Array.isArray(parsed.tabs) ? parsed.tabs : [])
      .map((tab) => normalizeTab(tab, projectRoot))
      .filter((tab): tab is TerminalTab => {
        if (!tab || seen.has(tab.clientId)) return false;
        seen.add(tab.clientId);
        return true;
      })
      .slice(0, MAX_TERMINAL_TABS);
    if (tabs.length === 0) return fallback();

    // A dedicated project-sandbox tab is the only implicit AI target. Repair older or malformed
    // layouts instead of allowing multiple tabs to compete for Agent input.
    let claimedAgentDefault = false;
    for (const tab of tabs) {
      tab.agentDefault =
        !claimedAgentDefault && tab.agentDefault && tab.executionMode === "project";
      claimedAgentDefault ||= tab.agentDefault;
    }
    if (!claimedAgentDefault) {
      const existingProjectTab = tabs.find((tab) => tab.executionMode === "project");
      if (existingProjectTab) {
        existingProjectTab.agentDefault = true;
      } else {
        const agentTab = {
          clientId: createTerminalClientId(),
          title: agentTitle,
          cwd: projectRoot,
          executionMode: "project" as const,
          agentDefault: true,
        };
        if (tabs.length < MAX_TERMINAL_TABS) tabs.push(agentTab);
        else tabs[tabs.length - 1] = agentTab;
      }
    }

    const activeClientId = tabs.some((tab) => tab.clientId === parsed.activeClientId)
      ? String(parsed.activeClientId)
      : tabs[0].clientId;
    return { tabs, activeClientId };
  } catch {
    return fallback();
  }
}

export function saveTerminalTabLayout(projectId: string, layout: TerminalTabLayout): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(`${STORAGE_PREFIX}${projectId}`, JSON.stringify(layout));
  } catch {
    // Running PTYs remain available even if the WebView cannot persist layout metadata.
  }
}
