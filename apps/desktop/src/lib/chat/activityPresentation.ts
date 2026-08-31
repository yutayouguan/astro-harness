import type { ChatActivity } from "../../types";
import type { MessageKey } from "../../i18n/messages";

export type ActivityVisualKind =
  | "read"
  | "search"
  | "run"
  | "edit"
  | "browse"
  | "image"
  | "video"
  | "music"
  | "speech"
  | "media"
  | "skill"
  | "mcp"
  | "hook"
  | "memory"
  | "status"
  | "tool";

type SemanticTitleKind =
  | "read"
  | "search"
  | "run"
  | "edit"
  | "browse"
  | "image"
  | "video"
  | "music"
  | "speech"
  | "media";

export type ActivityTitlePresentation = {
  key: MessageKey;
  target: string;
};

const SEARCH_NAMES = /(^|[_-])(search|grep|rg|find|glob|query)([_-]|$)/;
const READ_NAMES = /(^|[_-])(read|open|list|view|inspect|stat)([_-]|$)/;
const RUN_NAMES = /(^|[_-])(run|exec|execute|exec_command|shell|command|code_exec)([_-]|$)/;
const EDIT_NAMES = /(^|[_-])(edit|write|patch|apply|create|delete|remove|move|copy|rename)([_-]|$)/;
const BROWSE_NAMES = /(^|[_-])(browser|fetch|crawl|navigate|visit|http|url)([_-]|$)/;
const MEDIA_NAMES = /(^|[_-])(image|video|audio|media|render|generate)([_-]|$)/;
const IMAGE_GENERATION_NAMES =
  /(^|[_:-])(image[_-]?(gen|generate|generation)|generate[_-]?image)([_:-]|$)/;
const VIDEO_GENERATION_NAMES =
  /(^|[_:-])(video[_-]?(gen|generate|generation)|generate[_-]?video)([_:-]|$)/;
const MUSIC_GENERATION_NAMES =
  /(^|[_:-])(music[_-]?(gen|generate|generation)|generate[_-]?music)([_:-]|$)/;
const SPEECH_GENERATION_NAMES =
  /(^|[_:-])(speech[_-]?(gen|generate|generation)|audio[_-]?gen|tts|text[_-]?to[_-]?speech|voice[_-]?clone)([_:-]|$)/;

/** Map the native activity identity to a stable visual verb. */
export function activityVisualKind(activity: ChatActivity): ActivityVisualKind {
  if (activity.kind !== "tool") return activity.kind;

  const name = activity.title.trim().toLowerCase();
  const operation = activityOperation(activity.input);
  const candidates = [operation, name].filter(Boolean);
  // A generic operation such as `generate` must not hide the concrete tool
  // family carried by the tool name (`image_gen`, `video_gen`, ...).
  for (const candidate of candidates) {
    if (IMAGE_GENERATION_NAMES.test(candidate)) return "image";
    if (VIDEO_GENERATION_NAMES.test(candidate)) return "video";
    if (MUSIC_GENERATION_NAMES.test(candidate)) return "music";
    if (SPEECH_GENERATION_NAMES.test(candidate)) return "speech";
  }
  // Multiplexed tools such as exec_command/file_ops describe the real action
  // in their structured input; prefer it over the broad tool family name.
  for (const candidate of candidates) {
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
  const patchTarget = activityPatchTarget(activity.input);
  if (!input) return patchTarget;

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
  if (typeof raw !== "string") return patchTarget;

  const firstLine = raw.trim().split(/\r?\n/, 1)[0] ?? "";
  if (!firstLine) return "";
  const pathParts = firstLine.split(/[/\\]/).filter(Boolean);
  const compact =
    (kind === "read" || kind === "edit") && pathParts.length > 0
      ? pathParts[pathParts.length - 1]!
      : firstLine;
  return compact.length > 72 ? `${compact.slice(0, 69)}…` : compact;
}

export function activityTitlePresentation(
  activity: ChatActivity,
): ActivityTitlePresentation | null {
  const kind = activityVisualKind(activity);
  if (!isSemanticTitleKind(kind)) return null;
  // Media prompts can be long and visually unstable. Keep the summary terse;
  // the complete prompt remains available in the expanded input section.
  const target = isGeneratedMediaKind(kind) ? "" : activityDisplayTarget(activity);
  const namespace = target ? "item" : "action";
  const state = activity.status ? `.${activity.status}` : "";
  return {
    key: `chat.activity.${namespace}${state}.${kind}` as MessageKey,
    target,
  };
}

function isSemanticTitleKind(
  kind: ActivityVisualKind,
): kind is SemanticTitleKind {
  return (
    kind === "read" ||
    kind === "search" ||
    kind === "run" ||
    kind === "edit" ||
    kind === "browse" ||
    kind === "image" ||
    kind === "video" ||
    kind === "music" ||
    kind === "speech" ||
    kind === "media"
  );
}

function isGeneratedMediaKind(
  kind: ActivityVisualKind,
): kind is "image" | "video" | "music" | "speech" {
  return (
    kind === "image" ||
    kind === "video" ||
    kind === "music" ||
    kind === "speech"
  );
}

/** Extract the first edited file from apply_patch's freeform Lark payload. */
function activityPatchTarget(input: string | undefined): string {
  if (!input?.trim()) return "";

  let patch = input.trim();
  try {
    const parsed: unknown = JSON.parse(patch);
    if (typeof parsed === "string") {
      patch = parsed;
    } else if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
      const record = parsed as Record<string, unknown>;
      const candidate = record.patch ?? record.diff ?? record.input;
      if (typeof candidate === "string") patch = candidate;
    }
  } catch {
    // Freeform tool arguments are valid input even when they are not JSON.
  }

  const paths = [...patch.matchAll(/^\*\*\* (?:Add|Update|Delete) File:\s*(.+?)\s*$/gm)]
    .map((match) => match[1]?.trim() ?? "")
    .filter(Boolean);
  if (paths.length === 0) return "";

  const unique = [...new Set(paths)];
  const first = unique[0]!.replace(/\\/g, "/").split("/").filter(Boolean).pop()!;
  return unique.length > 1 ? `${first} +${unique.length - 1}` : first;
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

export type ActivityGroupSummary =
  | "research"
  | "inspect"
  | "modify_and_verify"
  | "modify"
  | "media"
  | "execute"
  | "coordinate"
  | "tools";

export type ActivityGroupProgress = {
  total: number;
  waiting: number;
  running: number;
  retrying: number;
  done: number;
  partial: number;
  error: number;
  interrupted: number;
  resolved: number;
  hasPartialOutcome: boolean;
};

export function activityGroupProgress(
  activities: ChatActivity[],
): ActivityGroupProgress {
  const count = (status: ChatActivity["status"]) =>
    activities.filter((activity) => activity.status === status).length;
  const waiting = count("waiting");
  const running = count("running");
  const retrying = count("retrying");
  const done = count("done");
  const partial = count("partial");
  const error = count("error");
  const interrupted = count("interrupted");
  const resolved = done + partial + error + interrupted;
  return {
    total: activities.length,
    waiting,
    running,
    retrying,
    done,
    partial,
    error,
    interrupted,
    resolved,
    hasPartialOutcome:
      partial > 0 || (done > 0 && (error > 0 || interrupted > 0)),
  };
}

/** Produce a stable semantic heading without adding another model request. */
export function activityGroupSummary(
  activities: ChatActivity[],
): ActivityGroupSummary {
  const kinds = new Set(distinctActivityVisualKinds(activities));
  if (kinds.has("edit") && kinds.has("run")) return "modify_and_verify";
  if (kinds.has("edit")) return "modify";
  if (
    kinds.has("media") ||
    (["image", "video", "music", "speech"] as const).some((kind) =>
      kinds.has(kind),
    )
  ) {
    return "media";
  }
  if (kinds.has("search") || kinds.has("browse")) return "research";
  if (kinds.has("run")) return "execute";
  if (kinds.has("read")) return "inspect";
  if (kinds.has("skill") || kinds.has("mcp") || kinds.has("hook")) {
    return "coordinate";
  }
  return "tools";
}
