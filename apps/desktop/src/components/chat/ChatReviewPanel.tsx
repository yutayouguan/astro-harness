import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { motion, useReducedMotion } from "framer-motion";
import { FileCode2, FileDiff, RefreshCw, X } from "lucide-react";
import type { FileChangeItem } from "../../lib/chat/taskProgress";
import { parseUnifiedDiff } from "../../lib/chat/unifiedDiff";

export type ProjectGitDiff = {
  path: string;
  relativePath: string;
  patch: string;
  additions: number;
  deletions: number;
  isBinary: boolean;
  tooLarge?: boolean;
};

export type ReviewDiffLoader = (
  projectId: string,
  path: string,
) => Promise<ProjectGitDiff>;

type Props = {
  projectId: string;
  files: FileChangeItem[];
  selectedPath: string;
  onSelectPath: (path: string) => void;
  onClose: () => void;
  loadDiff?: ReviewDiffLoader;
};

const defaultLoadDiff: ReviewDiffLoader = (projectId, path) =>
  invoke<ProjectGitDiff>("project_git_diff", { projectId, path });

function fileName(path: string): string {
  const parts = path.replace(/\\/g, "/").split("/").filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

function parentPath(path: string): string {
  const parts = path.replace(/\\/g, "/").split("/").filter(Boolean);
  return parts.slice(0, -1).join("/") || ".";
}

function frozenDiff(file: FileChangeItem): ProjectGitDiff | null {
  if (file.beforeContent == null && file.afterContent == null) return null;
  const before = file.beforeContent ?? "";
  const after = file.afterContent ?? "";
  if (before.length + after.length > 200_000) {
    return {
      path: file.path,
      relativePath: file.path,
      patch: "",
      additions: file.additions,
      deletions: file.deletions,
      isBinary: false,
      tooLarge: true,
    };
  }
  const beforeLines = before
    .replace(/\n$/, "")
    .split("\n")
    .filter((_, i) => before.length > 0 || i > 0);
  const afterLines = after
    .replace(/\n$/, "")
    .split("\n")
    .filter((_, i) => after.length > 0 || i > 0);
  const oldPath =
    file.beforeContent == null
      ? "/dev/null"
      : `a/${file.sourcePath ?? file.path}`;
  const newPath = file.afterContent == null ? "/dev/null" : `b/${file.path}`;
  const patch = [
    `diff --git a/${file.sourcePath ?? file.path} b/${file.path}`,
    `--- ${oldPath}`,
    `+++ ${newPath}`,
    `@@ -1,${beforeLines.length} +1,${afterLines.length} @@`,
    ...beforeLines.map((line) => `-${line}`),
    ...afterLines.map((line) => `+${line}`),
    "",
  ].join("\n");
  return {
    path: file.path,
    relativePath: file.path,
    patch,
    additions: file.additions,
    deletions: file.deletions,
    isBinary: false,
  };
}

export default function ChatReviewPanel({
  projectId,
  files,
  selectedPath,
  onSelectPath,
  onClose,
  loadDiff = defaultLoadDiff,
}: Props) {
  const reducedMotion = useReducedMotion();
  const [review, setReview] = useState<ProjectGitDiff | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [reloadKey, setReloadKey] = useState(0);
  const selectedFile = files.find((file) => file.path === selectedPath);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  useEffect(() => {
    let active = true;
    setLoading(true);
    setReview(null);
    setError(null);
    const frozen = selectedFile ? frozenDiff(selectedFile) : null;
    const request = frozen
      ? Promise.resolve(frozen)
      : loadDiff(projectId, selectedPath);
    request
      .then((next) => {
        if (!active) return;
        setReview(next);
      })
      .catch((reason) => {
        if (!active) return;
        setReview(null);
        setError(String(reason));
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [loadDiff, projectId, reloadKey, selectedFile, selectedPath]);

  const lines = useMemo(
    () => (review?.patch ? parseUnifiedDiff(review.patch) : []),
    [review?.patch],
  );
  const totalAdditions = files.reduce((sum, file) => sum + file.additions, 0);
  const totalDeletions = files.reduce((sum, file) => sum + file.deletions, 0);

  return (
    <motion.aside
      className="chat-review-panel"
      aria-label="审查文件改动"
      initial={
        reducedMotion ? { opacity: 0 } : { opacity: 0, x: 18, scale: 0.99 }
      }
      animate={{ opacity: 1, x: 0, scale: 1 }}
      exit={reducedMotion ? { opacity: 0 } : { opacity: 0, x: 18, scale: 0.99 }}
      transition={{
        duration: reducedMotion ? 0.12 : 0.24,
        ease: [0.22, 1, 0.36, 1],
      }}
    >
      <header className="chat-review-header">
        <div className="chat-review-title">
          <FileDiff size={17} strokeWidth={1.8} aria-hidden />
          <strong>审查</strong>
          <span className="chat-review-total">
            <b>+{totalAdditions}</b>
            <em>−{totalDeletions}</em>
          </span>
        </div>
        <div className="chat-review-header-actions">
          <button
            type="button"
            onClick={() => setReloadKey((key) => key + 1)}
            aria-label="刷新文件差异"
            title="刷新"
          >
            <RefreshCw size={14} strokeWidth={1.8} aria-hidden />
          </button>
          <button
            type="button"
            onClick={onClose}
            aria-label="关闭审查面板"
            title="关闭"
          >
            <X size={15} strokeWidth={1.8} aria-hidden />
          </button>
        </div>
      </header>

      <div className="chat-review-content">
        <section className="chat-review-diff" aria-label="文件差异">
          <header className="chat-review-file-header">
            <FileCode2 size={15} strokeWidth={1.8} aria-hidden />
            <span title={review?.relativePath ?? selectedPath}>
              {review?.relativePath ?? selectedPath}
            </span>
            {review && (
              <span className="chat-review-file-stat">
                <b>+{review.additions}</b>
                <em>−{review.deletions}</em>
              </span>
            )}
          </header>

          <div className="chat-review-code" aria-live="polite">
            {loading ? (
              <div className="chat-review-state">正在读取文件差异…</div>
            ) : error ? (
              <div className="chat-review-state is-error">
                <strong>无法打开文件差异</strong>
                <span>{error}</span>
              </div>
            ) : review?.tooLarge ? (
              <div className="chat-review-state">
                本轮冻结差异过大，请在编辑器中查看
              </div>
            ) : review?.isBinary ? (
              <div className="chat-review-state">
                二进制文件无法显示文本差异
              </div>
            ) : lines.length === 0 ? (
              <div className="chat-review-state">当前文件没有未提交改动</div>
            ) : (
              <div
                className="chat-review-lines"
                role="table"
                aria-label={fileName(selectedPath)}
              >
                {lines.map((line, index) => (
                  <div
                    key={`${index}-${line.kind}`}
                    className={`chat-review-line is-${line.kind}`}
                    role="row"
                  >
                    <span className="chat-review-line-number" role="cell">
                      {line.oldLine ?? ""}
                    </span>
                    <span className="chat-review-line-number" role="cell">
                      {line.newLine ?? ""}
                    </span>
                    <code role="cell">
                      {line.kind === "addition"
                        ? "+"
                        : line.kind === "deletion"
                          ? "−"
                          : " "}
                      {line.content}
                    </code>
                  </div>
                ))}
              </div>
            )}
          </div>
        </section>

        <nav className="chat-review-files" aria-label="已改动文件">
          <div className="chat-review-files-title">
            <span>文件改动</span>
            <span>{files.length}</span>
          </div>
          <div className="chat-review-file-list">
            {files.map((file) => (
              <button
                key={file.path}
                type="button"
                className={file.path === selectedPath ? "is-selected" : ""}
                onClick={() => onSelectPath(file.path)}
                title={file.path}
              >
                <FileCode2 size={14} strokeWidth={1.7} aria-hidden />
                <span className="chat-review-file-copy">
                  <strong>{fileName(file.path)}</strong>
                  <small>{parentPath(file.path)}</small>
                </span>
                <span className="chat-review-file-stat">
                  {file.additions > 0 && <b>+{file.additions}</b>}
                  {file.deletions > 0 && <em>−{file.deletions}</em>}
                </span>
              </button>
            ))}
          </div>
        </nav>
      </div>
    </motion.aside>
  );
}
