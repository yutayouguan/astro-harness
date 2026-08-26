/** 编辑项目弹窗：修改名称、管理源文件夹、图标选择、移除项目。 */
import { useCallback, useEffect, useId, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { ArrowDown, ArrowUp, FolderOpen, Plus, Trash2, X } from "lucide-react";
import type { ProjectDto } from "../../types";
import LucideIconPicker from "../agents/LucideIconPicker";
import LucideByName from "../icons/LucideByName";

type Props = {
  open: boolean;
  project: ProjectDto | null;
  onClose: () => void;
  /** 项目已更新（名称/文件夹变化） */
  onUpdated: (updated: ProjectDto) => void;
  /** 项目已移除 */
  onRemoved: (projectId: string) => void;
};

export default function ProjectEditDialog({
  open,
  project,
  onClose,
  onUpdated,
  onRemoved,
}: Props) {
  const titleId = useId();
  const [name, setName] = useState("");
  const [iconId, setIconId] = useState<string | null>(null);
  const [iconPickerOpen, setIconPickerOpen] = useState(false);
  const [roots, setRoots] = useState<string[]>([]);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (project) {
      setName(project.name);
      setIconId(project.icon ?? null);
      setRoots([...project.roots]);
    }
  }, [project]);

  const handleAddFolder = useCallback(async () => {
    try {
      const { open: pickDir } = await import("@tauri-apps/plugin-dialog");
      const selected = await pickDir({ directory: true, title: "选择源文件夹" });
      if (selected && typeof selected === "string") {
        setRoots((prev) => (prev.includes(selected) ? prev : [...prev, selected]));
      }
    } catch {
      /* 用户取消 */
    }
  }, []);

  const handleRemoveRoot = useCallback((path: string) => {
    setRoots((prev) => prev.filter((r) => r !== path));
  }, []);

  const moveRoot = useCallback((index: number, offset: -1 | 1) => {
    setRoots((prev) => {
      const target = index + offset;
      if (target < 0 || target >= prev.length) return prev;
      const next = [...prev];
      [next[index], next[target]] = [next[target], next[index]];
      return next;
    });
  }, []);

  const handleSave = useCallback(async () => {
    if (!project || saving) return;
    setSaving(true);
    try {
      const updated = await invoke<ProjectDto>("update_project", {
        projectId: project.id,
        name: name.trim() || project.name,
        icon: iconId ?? "",
        roots: project.id === "default" ? undefined : roots,
      });
      onUpdated(updated);
      onClose();
    } catch (err) {
      console.error("[ProjectEditDialog] save failed:", err);
      alert(`保存失败: ${err}`);
    } finally {
      setSaving(false);
    }
  }, [project, name, iconId, roots, saving, onUpdated, onClose]);

  const handleRemoveProject = useCallback(async () => {
    if (!project) return;
    try {
      await invoke("delete_project", { projectId: project.id });
      onRemoved(project.id);
      onClose();
    } catch {
      /* 移除失败静默 */
    }
  }, [project, onRemoved, onClose]);

  // ESC 关闭
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open || !project) return null;

  return createPortal(
    <div
      className="settings-overlay"
      onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}
      role="dialog"
      aria-modal="true"
      aria-labelledby={titleId}
    >
      <div className="project-edit-panel">
        <button
          type="button"
          className="settings-overlay-close"
          onClick={onClose}
          aria-label="Close"
        >
          <X size={16} strokeWidth={2} />
        </button>

        <h2 id={titleId} className="project-edit-title">编辑项目</h2>

        {/* 项目名称（图标内嵌在名称行左侧，点击即可换） */}
        <label className="project-edit-label">项目名称</label>
        <div className="project-edit-name-row">
          <button
            type="button"
            className="project-edit-name-icon-btn"
            onClick={() => setIconPickerOpen(true)}
            title="点击更换图标"
            aria-label="更换项目图标"
          >
            {iconId ? (
              <LucideByName name={iconId} size={16} />
            ) : (
              <FolderOpen size={16} strokeWidth={1.7} />
            )}
          </button>
          <input
            type="text"
            className="project-edit-name-input"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="项目名称"
          />
        </div>
        <LucideIconPicker
          open={iconPickerOpen}
          selectedId={iconId}
          onClose={() => setIconPickerOpen(false)}
          onSelect={(selected) => {
            setIconId(selected.id);
            setIconPickerOpen(false);
          }}
        />

        {/* 源文件夹列表 */}
        <label className="project-edit-label">源文件夹</label>
        <div className="project-edit-roots">
          {roots.length === 0 && (
            <div className="project-edit-roots-empty">暂无文件夹</div>
          )}
          {roots.map((r, index) => (
            <div key={r} className="project-edit-root-item">
              <span className="project-edit-root-path">{r}</span>
              {index === 0 && <span className="project-edit-root-primary">主目录</span>}
              <button
                type="button"
                className="project-edit-root-remove"
                onClick={() => moveRoot(index, -1)}
                disabled={index === 0}
                aria-label={`上移 ${r}`}
              >
                <ArrowUp size={13} strokeWidth={2} />
              </button>
              <button
                type="button"
                className="project-edit-root-remove"
                onClick={() => moveRoot(index, 1)}
                disabled={index === roots.length - 1}
                aria-label={`下移 ${r}`}
              >
                <ArrowDown size={13} strokeWidth={2} />
              </button>
              <button
                type="button"
                className="project-edit-root-remove"
                onClick={() => handleRemoveRoot(r)}
                disabled={roots.length === 1 || project.id === "default"}
                aria-label={`移除 ${r}`}
              >
                <X size={14} strokeWidth={2} />
              </button>
            </div>
          ))}
        </div>
        <button
          type="button"
          className="project-edit-add-folder"
          onClick={() => void handleAddFolder()}
          disabled={project.id === "default"}
        >
          <Plus size={14} strokeWidth={2} />
          <span>添加文件夹</span>
        </button>

        {/* 底部操作栏 */}
        <div className="project-edit-footer">
          <button
            type="button"
            className="project-edit-btn project-edit-btn--danger"
            onClick={() => void handleRemoveProject()}
            disabled={project.id === "default"}
          >
            <Trash2 size={14} strokeWidth={1.8} />
            <span>移除本地项目</span>
          </button>
          <div className="project-edit-footer-right">
            <button
              type="button"
              className="project-edit-btn project-edit-btn--secondary"
              onClick={onClose}
            >
              取消
            </button>
            <button
              type="button"
              className="project-edit-btn project-edit-btn--primary"
              onClick={() => void handleSave()}
              disabled={saving}
            >
              {saving ? "保存中…" : "保存"}
            </button>
          </div>
        </div>
      </div>
    </div>,
    document.body,
  );
}
