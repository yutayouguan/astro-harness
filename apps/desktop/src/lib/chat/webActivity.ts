import type { ChatWebAction } from "../../types";

type JsonRecord = Record<string, unknown>;

export type WebActivitySource = {
  name: string;
  input?: string | null;
  output?: string | null;
};

export type ChatWebActivityProjection = {
  action: ChatWebAction;
  pageTitle?: string;
};

/**
 * Project raw tool payloads into the Codex-style structured web action once,
 * at the live/history ingestion boundary. Presentation code must not parse IO.
 */
export function deriveChatWebActivity({
  name,
  input,
  output,
}: WebActivitySource): ChatWebActivityProjection | undefined {
  const normalizedName = name.trim().toLowerCase();
  if (!isWebToolName(normalizedName)) return undefined;

  const args = parseRecord(input);
  const result = parseRecord(output);
  const actionType = firstString(args?.type, args?.action, args?.operation)
    .replace(/_/g, "")
    .toLowerCase();

  if (isSearchTool(normalizedName) || actionType === "search") {
    const query = firstString(args?.query, args?.q);
    const queries = stringArray(args?.queries);
    return {
      action: {
        type: "search",
        ...(query ? { query } : {}),
        ...(queries.length > 0 ? { queries } : {}),
      },
    };
  }

  const snapshot = record(result?.snapshot);
  const activeTab = activeBrowserTab(result);
  const url = firstHttpUrl(
    result?.url,
    snapshot?.url,
    activeTab?.url,
    args?.url,
    args?.href,
    args?.uri,
  );
  const title = firstString(
    result?.title,
    snapshot?.title,
    activeTab?.title,
    args?.title,
  );
  const pattern = firstString(args?.pattern, args?.find, args?.text);

  if (actionType === "findinpage" || isFindInPageTool(normalizedName)) {
    return {
      action: {
        type: "findInPage",
        ...(url ? { url } : {}),
        ...(pattern ? { pattern } : {}),
      },
      ...(title ? { pageTitle: compactTarget(title) } : {}),
    };
  }
  if (!isOpenPageTool(normalizedName)) {
    return { action: { type: "other" } };
  }
  if (url) {
    return {
      action: { type: "openPage", url },
      ...(title ? { pageTitle: compactTarget(title) } : {}),
    };
  }
  return { action: { type: "other" } };
}

export function normalizeChatWebAction(
  value: unknown,
): ChatWebAction | undefined {
  const action = record(value);
  const type = firstString(action?.type);
  if (type === "search") {
    const query = firstString(action?.query);
    const queries = stringArray(action?.queries);
    return {
      type,
      ...(query ? { query } : {}),
      ...(queries.length > 0 ? { queries } : {}),
    };
  }
  if (type === "openPage") {
    const url = firstHttpUrl(action?.url);
    return {
      type,
      ...(url ? { url } : {}),
    };
  }
  if (type === "findInPage") {
    const url = firstHttpUrl(action?.url);
    const pattern = firstString(action?.pattern);
    return {
      type,
      ...(url ? { url } : {}),
      ...(pattern ? { pattern } : {}),
    };
  }
  return type === "other" ? { type } : undefined;
}

function isWebToolName(name: string): boolean {
  return /(^|[._:-])(browser|web|fetch|crawl|navigate|visit|http|url)([._:-]|$)/.test(
    name,
  );
}

function isSearchTool(name: string): boolean {
  return /(^|[._:-])(web[_-]?search|search)([._:-]|$)/.test(name);
}

function isFindInPageTool(name: string): boolean {
  return /(^|[._:-])(find[_-]?in[_-]?page|find)([._:-]|$)/.test(name);
}

function isOpenPageTool(name: string): boolean {
  const parts = name.split(/[._:-]/).filter(Boolean);
  const browserIndex = parts.indexOf("browser");
  const browserOperation =
    browserIndex >= 0 ? (parts[browserIndex + 1] ?? "") : "";
  return (
    [
      "open",
      "navigate",
      "snapshot",
      "screenshot",
      "back",
      "forward",
      "reload",
    ].includes(browserOperation) ||
    parts.some((part) =>
      ["fetch", "crawl", "navigate", "visit", "http", "url"].includes(part),
    )
  );
}

function parseRecord(raw: string | null | undefined): JsonRecord | null {
  if (!raw?.trim()) return null;
  try {
    return record(JSON.parse(raw));
  } catch {
    return null;
  }
}

function record(value: unknown): JsonRecord | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as JsonRecord)
    : null;
}

function activeBrowserTab(output: JsonRecord | null): JsonRecord | null {
  if (!Array.isArray(output?.tabs)) return null;
  const activeTabId = firstString(output.active_tab_id, output.activeTabId);
  const tabs = output.tabs.map(record).filter(Boolean) as JsonRecord[];
  return (
    tabs.find((tab) => tab.active === true) ??
    tabs.find((tab) => firstString(tab.id) === activeTabId) ??
    null
  );
}

function stringArray(value: unknown): string[] {
  return Array.isArray(value)
    ? value
        .filter((item): item is string => typeof item === "string")
        .map((item) => item.trim())
        .filter(Boolean)
    : [];
}

function firstString(...values: unknown[]): string {
  for (const value of values) {
    if (typeof value === "string" && value.trim()) return value.trim();
  }
  return "";
}

function firstHttpUrl(...values: unknown[]): string {
  for (const value of values) {
    if (typeof value !== "string") continue;
    const candidate = value.trim();
    if (/^https?:\/\/[^\s]+$/i.test(candidate)) return candidate;
  }
  return "";
}

function compactTarget(value: string): string {
  const firstLine = value.trim().split(/\r?\n/, 1)[0] ?? "";
  return firstLine.length > 72 ? `${firstLine.slice(0, 69)}…` : firstLine;
}
