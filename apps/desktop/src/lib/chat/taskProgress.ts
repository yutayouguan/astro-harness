import type { ChatActivity, ConversationEntry } from "../../types";

export type TodoPlanItem = {
  text: string;
  done: boolean;
};

export type TodoPlan = {
  title: string;
  planId: string;
  items: TodoPlanItem[];
};

export type FileChangeItem = {
  path: string;
  additions: number;
  deletions: number;
  sourcePath?: string;
  kind?: "add" | "update" | "delete" | "move";
  beforeContent?: string;
  afterContent?: string;
  reversible?: boolean;
};

export type FileChangeSummary = {
  items: FileChangeItem[];
  additions: number;
  deletions: number;
};

function normalizedToolName(title: string): string {
  const value = title.trim().toLowerCase();
  const colonParts = value.split(":");
  const namespaced = colonParts[colonParts.length - 1] ?? value;
  const toolParts = namespaced.split("__");
  return toolParts[toolParts.length - 1] ?? namespaced;
}

function parseInput(input: string | undefined): unknown {
  if (!input?.trim()) return null;
  try {
    return JSON.parse(input);
  } catch {
    return input;
  }
}

function objectValue(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function textValue(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function numberValue(value: unknown): number {
  const number = typeof value === "number" ? value : Number(value);
  return Number.isFinite(number) && number > 0 ? Math.floor(number) : 0;
}

function lineCount(value: string): number {
  if (!value) return 0;
  return value.replace(/\n$/, "").split("\n").length;
}

function cleanPath(value: string): string {
  return value.trim().replace(/^['"]|['"]$/g, "");
}

function addChange(
  changes: Map<string, FileChangeItem>,
  path: string,
  additions = 0,
  deletions = 0,
) {
  const normalizedPath = cleanPath(path);
  if (!normalizedPath) return;
  const current = changes.get(normalizedPath);
  if (current) {
    current.additions += additions;
    current.deletions += deletions;
    return;
  }
  changes.set(normalizedPath, {
    path: normalizedPath,
    additions,
    deletions,
  });
}

function patchTextFromInput(input: string | undefined): string {
  const parsed = parseInput(input);
  if (typeof parsed === "string") return parsed;
  const record = objectValue(parsed);
  return textValue(record?.input) || textValue(record?.patch);
}

function collectPatchChanges(
  changes: Map<string, FileChangeItem>,
  patch: string,
) {
  let path = "";
  let additions = 0;
  let deletions = 0;

  const flush = () => {
    if (path) addChange(changes, path, additions, deletions);
    path = "";
    additions = 0;
    deletions = 0;
  };

  for (const line of patch.split("\n")) {
    const fileMatch = line.match(/^\*\*\* (?:Add|Update|Delete) File:\s*(.+)$/);
    if (fileMatch) {
      flush();
      path = fileMatch[1] ?? "";
      continue;
    }
    const moveMatch = line.match(/^\*\*\* Move to:\s*(.+)$/);
    if (moveMatch && path) {
      path = moveMatch[1] ?? path;
      continue;
    }
    if (!path || line.startsWith("***")) continue;
    if (line.startsWith("+")) additions += 1;
    else if (line.startsWith("-")) deletions += 1;
  }
  flush();
}

function collectStructuredFileChange(
  changes: Map<string, FileChangeItem>,
  activity: ChatActivity,
) {
  const record = objectValue(parseInput(activity.input));
  if (!record) return;
  const namedOperation = normalizedToolName(activity.title).replace(
    /_file$/,
    "",
  );
  const operation =
    textValue(record.operation || record.action).toLowerCase() ||
    namedOperation;
  const patch = textValue(record.patch || record.diff);
  if (patch.includes("*** Begin Patch")) {
    collectPatchChanges(changes, patch);
    return;
  }

  const path = textValue(
    record.path || record.file_path || record.file || record.target,
  );
  if (
    !path ||
    !["write", "append", "edit", "patch", "delete", "create"].includes(
      operation,
    )
  ) {
    return;
  }
  const content = textValue(record.content || record.text || record.data);
  const additions = ["write", "append", "create"].includes(operation)
    ? lineCount(content)
    : 0;
  addChange(changes, path, additions, 0);
}

function collectFileChangeEvent(
  changes: Map<string, FileChangeItem>,
  activity: ChatActivity,
) {
  const record =
    objectValue(parseInput(activity.input)) ??
    objectValue(parseInput(activity.output));
  if (!record) return;
  const files = Array.isArray(record.files) ? record.files : [record];
  for (const value of files) {
    const file = objectValue(value);
    if (!file) continue;
    addChange(
      changes,
      textValue(file.path || file.file_path || file.file),
      numberValue(file.additions || file.added),
      numberValue(file.deletions || file.deleted),
    );
  }
}

/** TODO 是输入框状态源，不应再作为回答时间线里的普通工具卡展示。 */
export function isTodoActivity(activity: ChatActivity): boolean {
  return normalizedToolName(activity.title) === "todo";
}

/** 仅承载 TODO 更新的助手消息可以从回答列表完全省略。 */
export function isTodoOnlyActivityMessage(message: ConversationEntry): boolean {
  return (
    message.role === "assistant" &&
    !message.content.trim() &&
    !message.reasoning?.trim() &&
    !message.attachments?.length &&
    !message.uiSurfaces?.length &&
    Boolean(message.activities?.length) &&
    message.activities!.every(isTodoActivity)
  );
}

function isNewTodoPlan(activity: ChatActivity): boolean {
  if (!isTodoActivity(activity)) return false;
  const record = objectValue(parseInput(activity.input));
  return textValue(record?.action).toLowerCase() !== "update";
}

// 扫描助手消息的 activities，提取最后一个 todo 工具调用的计划状态。
export function extractLatestTodoPlan(
  messages: ConversationEntry[],
): TodoPlan | null {
  let latest: TodoPlan | null = null;

  for (const message of messages) {
    if (message.role !== "assistant" || !message.activities) continue;
    for (const activity of message.activities) {
      if (!isTodoActivity(activity)) continue;
      const args = objectValue(parseInput(activity.input));
      if (!args || !Array.isArray(args.items) || args.items.length === 0)
        continue;
      const items: TodoPlanItem[] = args.items.flatMap((item) => {
        if (typeof item === "string") return [{ text: item, done: false }];
        const record = objectValue(item);
        const text = textValue(record?.text).trim();
        return text ? [{ text, done: Boolean(record?.done) }] : [];
      });
      if (items.length === 0) continue;
      const outputPlanId = activity.output?.match(
        /([0-9]{8}-[a-f0-9]{6})/,
      )?.[1];
      latest = {
        title: textValue(args.title) || "Todo",
        planId: textValue(args.plan_id) || outputPlanId || "",
        items,
      };
    }
  }

  return latest;
}

/** 聚合最近一次新建 TODO 后的显式文件写入记录。 */
export function extractFileChangeSummary(
  messages: ConversationEntry[],
): FileChangeSummary {
  const changes = new Map<string, FileChangeItem>();

  for (const message of messages) {
    if (message.role !== "assistant" || !message.activities) continue;
    for (const activity of message.activities) {
      if (isNewTodoPlan(activity)) {
        changes.clear();
        continue;
      }
      const name = normalizedToolName(activity.title);
      if (name === "apply_patch") {
        collectPatchChanges(changes, patchTextFromInput(activity.input));
      } else if (name === "file_change") {
        collectFileChangeEvent(changes, activity);
      } else if (
        name === "file_ops" ||
        name === "write_file" ||
        name === "edit_file" ||
        name === "delete_file" ||
        name === "create_file"
      ) {
        collectStructuredFileChange(changes, activity);
      }
    }
  }

  const items = [...changes.values()];
  return {
    items,
    additions: items.reduce((sum, item) => sum + item.additions, 0),
    deletions: items.reduce((sum, item) => sum + item.deletions, 0),
  };
}

/** 聚合单个 assistant turn 的结构化净变更。 */
export function extractTurnFileChangeSummary(
  message: ConversationEntry,
): FileChangeSummary {
  const changes = new Map<string, FileChangeItem>();
  for (const activity of message.activities ?? []) {
    for (const change of activity.fileChanges ?? []) {
      const path = change.move_path || change.path;
      const current = changes.get(path);
      if (current) {
        current.afterContent = change.after_content;
        current.additions += change.additions;
        current.deletions += change.deletions;
        current.reversible = current.reversible === true && change.reversible;
        current.kind = change.kind;
        continue;
      }
      changes.set(path, {
        path,
        sourcePath: change.path,
        kind: change.kind,
        beforeContent: change.before_content,
        afterContent: change.after_content,
        additions: change.additions,
        deletions: change.deletions,
        reversible: change.reversible,
      });
    }
  }
  const items = [...changes.values()];
  return {
    items,
    additions: items.reduce((sum, item) => sum + item.additions, 0),
    deletions: items.reduce((sum, item) => sum + item.deletions, 0),
  };
}

export function displayFileName(path: string): string {
  const parts = path.replace(/\\/g, "/").split("/").filter(Boolean);
  return parts[parts.length - 1] || path;
}
