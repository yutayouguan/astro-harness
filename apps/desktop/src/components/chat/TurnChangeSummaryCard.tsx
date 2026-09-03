import { useMemo, useState } from "react";
import { ChevronDown, FileDiff, RotateCcw } from "lucide-react";
import type { ConversationEntry } from "../../types";
import {
  displayFileName,
  extractTurnFileChangeSummary,
  type FileChangeItem,
} from "../../lib/chat/taskProgress";

type Props = {
  message: ConversationEntry;
  onReview?: (file: FileChangeItem, files: FileChangeItem[]) => void;
};

export default function TurnChangeSummaryCard({ message, onReview }: Props) {
  const summary = useMemo(() => extractTurnFileChangeSummary(message), [message]);
  const [expanded, setExpanded] = useState(false);
  if (summary.items.length === 0) return null;
  const visible = expanded ? summary.items : summary.items.slice(0, 3);
  const remaining = summary.items.length - visible.length;
  const reversible = summary.items.every((item) => item.reversible === true);

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
        <span className="turn-change-undo-hint">
          <RotateCcw size={12} aria-hidden /> 已保存可撤销快照
        </span>
      ) : null}
    </section>
  );
}
