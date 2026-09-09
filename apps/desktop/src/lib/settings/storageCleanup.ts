export type CleanupItem = {
  path: string;
  bytes: number;
  policy: "cache_expired" | "logs_30_days";
};
export type CleanupPlan = {
  token: string;
  rootPath: string;
  expiresAtMs: number;
  items: CleanupItem[];
  totalBytes: number;
  omittedFiles: boolean;
};
export type CleanupResult = {
  batchId: string;
  recoveryPath: string;
  movedFiles: number;
  movedBytes: number;
  unverifiedFiles: number;
  manifestComplete: boolean;
  outcomes: { path: string; status: string }[];
};

const object = (value: unknown): value is Record<string, unknown> =>
  value !== null && typeof value === "object";
const count = (value: unknown): value is number =>
  typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
const hiddenControls = /[\u0000-\u001f\u007f\u202a-\u202e\u2066-\u2069]/;
export const cleanupToken = (value: unknown): value is string =>
  typeof value === "string" &&
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(value);
const relative = (path: unknown): path is string =>
  typeof path === "string" &&
  !hiddenControls.test(path) &&
  path.length > 0 &&
  !/^(?:[a-z]:|[\\/])/i.test(path) &&
  path.split(/[\\/]/).every((p) => p !== ".." && p !== "." && p !== "");

export function parseCleanupPlan(value: unknown): CleanupPlan {
  if (
    !object(value) ||
    !cleanupToken(value.token) ||
    typeof value.rootPath !== "string" ||
    hiddenControls.test(value.rootPath) ||
    !count(value.expiresAtMs) ||
    !count(value.totalBytes) ||
    typeof value.omittedFiles !== "boolean" ||
    !Array.isArray(value.items) ||
    value.items.length < 1 ||
    value.items.length > 20 ||
    !value.items.every(
      (i) =>
        object(i) &&
        relative(i.path) &&
        count(i.bytes) &&
        ["cache_expired", "logs_30_days"].includes(i.policy as string),
    )
  )
    throw new Error("cleanup_invalid_plan");
  const plan = value as CleanupPlan;
  if (
    new Set(plan.items.map((i) => i.path)).size !== plan.items.length ||
    plan.items.reduce((n, i) => n + i.bytes, 0) !== plan.totalBytes
  )
    throw new Error("cleanup_invalid_plan");
  return plan;
}

export function parseCleanupResult(value: unknown): CleanupResult {
  if (
    !object(value) ||
    !cleanupToken(value.batchId) ||
    typeof value.recoveryPath !== "string" ||
    !count(value.movedFiles) ||
    !count(value.movedBytes) ||
    !count(value.unverifiedFiles) ||
    typeof value.manifestComplete !== "boolean" ||
    !Array.isArray(value.outcomes) ||
    !value.outcomes.every(
      (i) =>
        object(i) &&
        relative(i.path) &&
        [
          "moved",
          "changed_not_moved",
          "move_failed",
          "changed_in_recovery",
          "not_attempted",
        ].includes(i.status as string),
    )
  )
    throw new Error("cleanup_invalid_result");
  const result = value as CleanupResult;
  if (
    result.outcomes.length > 20 ||
    new Set(result.outcomes.map((i) => i.path)).size !==
      result.outcomes.length ||
    result.movedFiles !==
      result.outcomes.filter((i) => i.status === "moved").length ||
    result.unverifiedFiles !==
      result.outcomes.filter((i) => i.status === "changed_in_recovery")
        .length ||
    result.movedBytes > 32 * 1024 * 1024 ||
    hiddenControls.test(result.recoveryPath)
  )
    throw new Error("cleanup_invalid_result");
  return result;
}

export function canConfirmCleanup(
  plan: CleanupPlan | null,
  acknowledged: boolean,
  busy: boolean,
  now = Date.now(),
): boolean {
  return (
    !!plan &&
    acknowledged &&
    !busy &&
    now < plan.expiresAtMs &&
    plan.items.length > 0 &&
    plan.items.length <= 20
  );
}

export const cleanupCopy = {
  zh: {
    prepare: "生成清理确认单",
    preparing: "正在核对文件…",
    moving: "正在移入恢复区…",
    title: "确认移入恢复区",
    explanation:
      "仅移动下列文件，不永久删除。文件会保存在数据根的 backups/storage-cleanup/ 中；磁盘空间不会立即释放。",
    limits:
      "确认单 5 分钟内有效，每次最多 20 个文件。配置或文件变化后需要重新生成。",
    omitted: "可能仍有未检查或未纳入的文件；本次只处理所列清单。",
    acknowledge: "我已核对完整清单，同意移动以上文件",
    confirm: "确认移入恢复区",
    cancel: "取消",
    reveal: "查看恢复区",
    files: "个文件",
    root: "数据根",
    complete: "已移入恢复区",
    partial: "部分条目需检查恢复清单",
    verified: "已验证移动",
    unverified: "发生变化并保留在恢复区",
    bytes: "文件大小",
    recover:
      "恢复清单记录了原路径和 SHA-256。恢复前关闭应用，不要覆盖原位置的新文件；应用重启后仍可在 backups/storage-cleanup/ 查找记录。",
    manifestWarning:
      "清单更新未完全确认。已创建的初始清单和恢复文件仍保留；请核对实物，不要直接重试覆盖。",
    errors: {
      cleanup_unsafe_path: "路径或软链接无法安全验证，操作已停止。",
      cleanup_unavailable:
        "无法完成操作，请刷新检查；若已开始移动，请查看恢复区。",
      cleanup_not_ready: "配置或迁移状态不允许清理，请先处理诊断问题。",
      cleanup_no_candidates:
        "没有可安全移动的条目；文件可能已变化、过大或无法验证。",
      cleanup_unknown_plan: "确认单已使用或已失效，请重新生成。",
      cleanup_expired: "确认单已过期，请重新生成。",
      cleanup_changed: "文件、目录或配置已变化，旧确认不能继续使用。",
      cleanup_busy: "相关文件正在操作或有其他确认单占用，请稍后重试。",
      cleanup_budget: "本次文件超过安全读取预算，请分批处理。",
      cleanup_confirmation_required: "请先核对并确认清单。",
      cleanup_invalid_plan: "确认单数据无效，未执行移动。",
      cleanup_invalid_result: "无法读取操作结果，请核对恢复区清单。",
    },
    statuses: {
      moved: "已移动",
      changed_not_moved: "已变化，未移动",
      move_failed: "移动未确认，请核对原位与恢复区",
      changed_in_recovery: "发生变化，文件保留在恢复区",
      not_attempted: "未处理",
    },
  },
  en: {
    prepare: "Prepare cleanup confirmation",
    preparing: "Verifying files…",
    moving: "Moving to recovery…",
    title: "Confirm move to recovery",
    explanation:
      "Only the listed files will move. Nothing is permanently deleted. Files stay in backups/storage-cleanup/ under the data root; disk space is not immediately freed.",
    limits:
      "Valid for 5 minutes, up to 20 files per batch. Changed files or configuration require a new plan.",
    omitted:
      "Other files may remain unchecked or excluded. Only this list will be processed.",
    acknowledge: "I reviewed the complete list and approve moving these files",
    confirm: "Confirm move to recovery",
    cancel: "Cancel",
    reveal: "Show recovery folder",
    files: "files",
    root: "Data root",
    complete: "Moved to recovery",
    partial: "Some entries need recovery review",
    verified: "Verified moves",
    unverified: "Changed files retained in recovery",
    bytes: "File size",
    recover:
      "The manifest records original paths and SHA-256. Stop the app before restoring; never overwrite newer files. Records remain in backups/storage-cleanup/ after restarting.",
    manifestWarning:
      "The final manifest update was not fully confirmed. The initial manifest and recovery files remain. Inspect them before retrying.",
    errors: {
      cleanup_unsafe_path: "Unsafe path or symlink. Operation stopped.",
      cleanup_unavailable:
        "Operation unavailable. Refresh inspection; if movement started, review recovery.",
      cleanup_not_ready: "Resolve configuration or migration issues first.",
      cleanup_no_candidates:
        "No safely movable files. They may have changed, exceed the budget, or be unverifiable.",
      cleanup_unknown_plan:
        "The plan was consumed or invalidated. Prepare a new one.",
      cleanup_expired: "The plan expired. Prepare a new one.",
      cleanup_changed:
        "Files, directories or settings changed. The old confirmation is invalid.",
      cleanup_busy:
        "Files or other pending confirmations are busy. Retry later.",
      cleanup_budget: "The safe read budget was exceeded. Use smaller batches.",
      cleanup_confirmation_required: "Review and confirm the list first.",
      cleanup_invalid_plan: "Invalid confirmation data. No move was requested.",
      cleanup_invalid_result:
        "Cannot read the result. Inspect the recovery manifest.",
    },
    statuses: {
      moved: "Moved",
      changed_not_moved: "Changed; not moved",
      move_failed: "Move not confirmed; inspect source and recovery",
      changed_in_recovery: "Changed; retained in recovery",
      not_attempted: "Not attempted",
    },
  },
} as const;
