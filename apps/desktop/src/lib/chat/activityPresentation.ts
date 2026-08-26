import type { ChatActivity } from "../../types";

export type ActivityVisualKind =
  | "read"
  | "search"
  | "run"
  | "edit"
  | "browse"
  | "media"
  | "skill"
  | "mcp"
  | "hook"
  | "memory"
  | "status"
  | "tool";

const SEARCH_NAMES = /(^|[_-])(search|grep|rg|find|glob|query)([_-]|$)/;
const READ_NAMES = /(^|[_-])(read|open|list|view|inspect|stat)([_-]|$)/;
const RUN_NAMES = /(^|[_-])(run|exec|execute|terminal|shell|command|code_exec)([_-]|$)/;
const EDIT_NAMES = /(^|[_-])(edit|write|patch|apply|create|delete|remove|move|copy|rename)([_-]|$)/;
const BROWSE_NAMES = /(^|[_-])(browser|fetch|crawl|navigate|visit|http|url)([_-]|$)/;
const MEDIA_NAMES = /(^|[_-])(image|video|audio|media|render|generate)([_-]|$)/;

/** Map the native activity identity to a stable visual verb. */
export function activityVisualKind(activity: ChatActivity): ActivityVisualKind {
  if (activity.kind !== "tool") return activity.kind;

  const name = activity.title.trim().toLowerCase();
  const operation = activityOperation(activity.input);
  for (const candidate of [name, operation]) {
    if (!candidate) continue;
    if (SEARCH_NAMES.test(candidate)) return "search";
    if (READ_NAMES.test(candidate)) return "read";
    if (RUN_NAMES.test(candidate)) return "run";
    if (EDIT_NAMES.test(candidate)) return "edit";
    if (BROWSE_NAMES.test(candidate)) return "browse";
    if (MEDIA_NAMES.test(candidate)) return "media";
  }
  return "tool";
}

/** Extract a compact subject for the human-readable activity row. */
export function activityDisplayTarget(activity: ChatActivity): string {
  const input = activityInput(activity.input);
  if (!input) return "";

  const kind = activityVisualKind(activity);
  const raw =
    kind === "search"
      ? input.query ?? input.pattern ?? input.path
      : kind === "run"
        ? input.command ?? input.cmd ?? input.code
        : kind === "browse"
          ? input.url ?? input.href
          : kind === "media"
            ? input.prompt ?? input.path
            : input.path ?? input.file ?? input.target;
  if (typeof raw !== "string") return "";

  const firstLine = raw.trim().split(/\r?\n/, 1)[0] ?? "";
  if (!firstLine) return "";
  const pathParts = firstLine.split(/[/\\]/).filter(Boolean);
  const compact =
    (kind === "read" || kind === "edit") && pathParts.length > 0
      ? pathParts[pathParts.length - 1]!
      : firstLine;
  return compact.length > 72 ? `${compact.slice(0, 69)}…` : compact;
}

function activityOperation(input: string | undefined): string {
  const value = activityInput(input);
  const operation = value?.operation ?? value?.action ?? value?.mode;
  return typeof operation === "string" ? operation.trim().toLowerCase() : "";
}

function activityInput(input: string | undefined): Record<string, unknown> | null {
  if (!input?.trim()) return null;
  try {
    const value = JSON.parse(input);
    return value && typeof value === "object" && !Array.isArray(value)
      ? (value as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
}

export function distinctActivityVisualKinds(
  activities: ChatActivity[],
): ActivityVisualKind[] {
  const kinds = new Set<ActivityVisualKind>();
  for (const activity of activities) kinds.add(activityVisualKind(activity));
  return [...kinds];
}
