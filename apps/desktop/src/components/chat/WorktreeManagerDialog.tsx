import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { GitBranch, RefreshCw, Trash2, X } from "lucide-react";

export type ManagedWorktreeDto = {
  id: string;
  path: string;
  branch?: string | null;
  headSha: string;
  ownerSessionId?: string | null;
  dirty: boolean;
};

type Props = {
  projectName: string;
  projectRoot: string;
  onClose: () => void;
};

export default function WorktreeManagerDialog({
  projectName,
  projectRoot,
  onClose,
}: Props) {
  const [items, setItems] = useState<ManagedWorktreeDto[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setItems(
        await invoke<ManagedWorktreeDto[]>("list_task_worktrees", {
          projectRoot,
        }),
      );
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setLoading(false);
    }
  }, [projectRoot]);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  const remove = useCallback(
    async (worktree: ManagedWorktreeDto) => {
      if (worktree.dirty) return;
      if (!window.confirm(`删除工作树 ${worktree.path}？`)) return;
      try {
        const removed = await invoke<boolean>("cleanup_task_worktree", {
          worktreeId: worktree.id,
        });
        if (!removed) {
          setError("工作树包含未提交或忽略文件，已保留供恢复。");
        }
        await load();
      } catch (cause) {
        setError(cause instanceof Error ? cause.message : String(cause));
      }
    },
    [load],
  );

  return (
    <div
      className="app-dialog-backdrop"
      role="presentation"
      onMouseDown={onClose}
    >
      <section
        className="app-dialog worktree-manager-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="worktree-manager-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="app-dialog-head">
          <span className="app-dialog-icon" aria-hidden>
            <GitBranch size={18} />
          </span>
          <div className="app-dialog-copy">
            <h3 id="worktree-manager-title">工作树</h3>
            <p>{projectName}</p>
          </div>
          <button
            type="button"
            className="app-dialog-btn is-cancel"
            onClick={onClose}
          >
            <X size={15} aria-hidden />
            关闭
          </button>
        </header>
        <div className="app-dialog-body worktree-manager-body">
          <div className="worktree-manager-toolbar">
            <span>{projectRoot}</span>
            <button
              type="button"
              className="app-dialog-btn is-cancel"
              onClick={() => void load()}
            >
              <RefreshCw size={14} aria-hidden />
              刷新
            </button>
          </div>
          {error ? <p className="worktree-manager-error">{error}</p> : null}
          {loading ? (
            <p>正在读取工作树…</p>
          ) : items.length === 0 ? (
            <p>当前项目没有可恢复的 Astro 工作树。</p>
          ) : (
            <ul className="worktree-manager-list">
              {items.map((item) => (
                <li key={item.id} className="worktree-manager-item">
                  <div>
                    <strong>{item.branch || "detached"}</strong>
                    <span>{item.path}</span>
                    <small>
                      {item.ownerSessionId
                        ? `会话 ${item.ownerSessionId}`
                        : "未绑定会话"}
                      {item.dirty
                        ? " · 有本地改动，禁止自动删除"
                        : " · 可安全清理"}
                    </small>
                  </div>
                  <button
                    type="button"
                    className="app-dialog-btn is-confirm is-danger"
                    disabled={item.dirty}
                    onClick={() => void remove(item)}
                  >
                    <Trash2 size={14} aria-hidden />
                    删除
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      </section>
    </div>
  );
}
