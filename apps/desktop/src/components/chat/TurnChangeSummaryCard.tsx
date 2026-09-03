import { useMemo, useState } from "react";
import { ChevronDown, FileDiff, RotateCcw } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import type { ConversationEntry } from "../../types";
import {
  displayFileName,
  extractTurnFileChangeSummary,
  type FileChangeItem,
} from "../../lib/chat/taskProgress";

type Props = {
  message: ConversationEntry;
  projectId?: string | null;
  onReview?: (file: FileChangeItem, files: FileChangeItem[]) => void;
};

export default function TurnChangeSummaryCard({ message, projectId, onReview }: Props) {
  const summary = useMemo(() => extractTurnFileChangeSummary(message), [message]);
  const [expanded, setExpanded] = useState(false);
  const [undone, setUndone] = useState(false);
  const [applying, setApplying] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  if (summary.items.length === 0) return null;
  const visible = expanded ? summary.items : summary.items.slice(0, 3);
  const remaining = summary.items.length - visible.length;
  const reversible = summary.items.every((item) => item.reversible === true);
  const applyChanges = async () => {
    if (!projectId || !reversible || applying) return;
    setApplying(true);
    setActionError(null);
    try {
      const result = await invoke<{ status: string; conflictedPaths: string[] }>(
        "apply_turn_file_changes",
        { projectId, changes: summary.items, revert: !undone },
      );
      if (result.status !== "success") {
        setActionError(`文件已变化，未覆盖：${result.conflictedPaths.join("、")}`);
        return;
      }
      setUndone((value) => !value);
    } catch (error) {
      setActionError(String(error));
    } finally {
      setApplying(false);
    }
  };

  return (
    <section className="turn-change-card" aria-label="本轮文件改动">
      <header className="turn-change-card-header">
        <span className="turn-change-card-icon" aria-hidden>
          <FileDiff size={18} strokeWidth={1.8} />
        </span>
        <span className="turn-change-card-title">
          <strong>已编辑 {summary.items.length} 个文件</strong>
          <span>
            <b>+{summary.additions}</b> <em>−{summary.deletions}</em>
          </span>
        </span>
        {!reversible ? (
          <span className="turn-change-card-note" title="部分修改缺少精确快照">
            部分不可撤销
          </span>
        ) : null}
        <button
          type="button"
          className="turn-change-review-btn"
          disabled={!onReview}
          onClick={() => onReview?.(summary.items[0]!, summary.items)}
        >
          审核
        </button>
      </header>
      <div className="turn-change-file-list">
        {visible.map((file) => (
          <button
            key={`${file.sourcePath ?? file.path}:${file.path}`}
            type="button"
            onClick={() => onReview?.(file, summary.items)}
          >
            <span title={file.path}>{displayFileName(file.path)}</span>
            <span>
              {file.additions ? <b>+{file.additions}</b> : null}
              {file.deletions ? <em>−{file.deletions}</em> : null}
            </span>
          </button>
        ))}
        {remaining > 0 || expanded ? (
          <button
            type="button"
            className="turn-change-expand"
            onClick={() => setExpanded((value) => !value)}
          >
            <ChevronDown
              size={14}
              className={expanded ? "is-expanded" : ""}
              aria-hidden
            />
            {expanded ? "收起文件" : `再显示 ${remaining} 个文件`}
          </button>
        ) : null}
      </div>
      {reversible ? (
        <button
          type="button"
          className="turn-change-undo-hint"
          disabled={!projectId || applying}
          onClick={() => void applyChanges()}
        >
          <RotateCcw size={12} aria-hidden />
          {applying ? "正在应用…" : undone ? "重新应用" : "撤销本轮修改"}
        </button>
      ) : null}
      {actionError ? <p className="turn-change-action-error">{actionError}</p> : null}
    </section>
  );
}
