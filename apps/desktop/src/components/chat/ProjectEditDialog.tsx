/** 创建 / 编辑项目弹窗：管理名称、源文件夹与图标。 */
import { useCallback, useEffect, useId, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ArrowDown, ArrowUp, Plus, Trash2, X } from "lucide-react";
import type { ProjectDto } from "../../types";
import { ModalShell } from "../ui";
import ProjectFolderIcon from "./ProjectFolderIcon";
import ProjectFolderIconPicker from "./ProjectFolderIconPicker";

type Props = {
  open: boolean;
  project: ProjectDto | null;
  onClose: () => void;
  /** 项目已更新（名称/文件夹变化） */
  onUpdated: (updated: ProjectDto) => void;
  /** 项目已创建 */
  onCreated: (created: ProjectDto) => void;
  /** 项目已移除 */
  onRemoved: (projectId: string) => void;
};

export default function ProjectEditDialog({
  open,
  project,
  onClose,
  onUpdated,
  onCreated,
  onRemoved,
}: Props) {
  const titleId = useId();
  const [name, setName] = useState("");
  const [iconId, setIconId] = useState<string | null>(null);
  const [iconPickerOpen, setIconPickerOpen] = useState(false);
  const [roots, setRoots] = useState<string[]>([]);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (!open) return;
    if (project) {
      setName(project.name);
      setIconId(project.icon ?? null);
      setRoots([...project.roots]);
    } else {
      setName("");
      setIconId(null);
      setRoots([]);
    }
    setIconPickerOpen(false);
  }, [open, project]);

  const handleAddFolder = useCallback(async () => {
    try {
      const { open: pickDir } = await import("@tauri-apps/plugin-dialog");
      const selected = await pickDir({
        directory: true,
        title: "选择源文件夹",
      });
      if (selected && typeof selected === "string") {
        setRoots((prev) =>
          prev.includes(selected) ? prev : [...prev, selected],
        );
        if (!project && !name.trim()) {
          const inferredName = selected.split(/[\\/]/).filter(Boolean).pop();
          if (inferredName) setName(inferredName);
        }
      }
    } catch {
      /* 用户取消 */
    }
  }, [project, name]);

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
    if (saving) return;
    const trimmedName = name.trim();
    if (!trimmedName || roots.length === 0) return;
    setSaving(true);
    try {
      if (project) {
        const updated = await invoke<ProjectDto>("update_project", {
          projectId: project.id,
          name: trimmedName,
          icon: iconId ?? "",
          roots: project.id === "default" ? undefined : roots,
        });
        onUpdated(updated);
      } else {
        const created = await invoke<ProjectDto>("create_project", {
          name: trimmedName,
          icon: iconId,
          roots,
        });
        onCreated(created);
      }
      onClose();
    } catch (err) {
      console.error("[ProjectEditDialog] save failed:", err);
      alert(`保存失败: ${err}`);
    } finally {
      setSaving(false);
    }
  }, [project, name, iconId, roots, saving, onUpdated, onCreated, onClose]);

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

  const isDefaultProject = project?.id === "default";
  const isCreating = project === null;

  return (
    <ModalShell
      open={open}
      onClose={onClose}
      closeOnEscape={!iconPickerOpen}
      className="project-edit-panel"
      aria-labelledby={titleId}
    >
      <button
        type="button"
        className="settings-overlay-close"
        onClick={onClose}
        aria-label="Close"
      >
        <X size={16} strokeWidth={2} />
      </button>

      <h2 id={titleId} className="project-edit-title">
        {isCreating ? "创建项目" : "编辑项目"}
      </h2>

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
          <ProjectFolderIcon iconId={iconId} expanded size={22} />
        </button>
        <input
          type="text"
          className="project-edit-name-input"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="项目名称"
        />
      </div>
      <ProjectFolderIconPicker
        open={iconPickerOpen}
        selectedId={iconId}
        onClose={() => setIconPickerOpen(false)}
        onSelect={(selectedId) => {
          setIconId(selectedId);
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
            {index === 0 && (
              <span className="project-edit-root-primary">主目录</span>
            )}
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
              disabled={isDefaultProject || (!isCreating && roots.length === 1)}
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
        disabled={isDefaultProject}
      >
        <Plus size={14} strokeWidth={2} />
        <span>添加文件夹</span>
      </button>

      {/* 底部操作栏 */}
      <div className="project-edit-footer">
        {isCreating ? (
          <span className="project-edit-protected">
            设置名称、图标和项目文件夹
          </span>
        ) : isDefaultProject ? (
          <span className="project-edit-protected">主空间不可移除</span>
        ) : (
          <button
            type="button"
            className="project-edit-btn project-edit-btn--danger"
            onClick={() => void handleRemoveProject()}
          >
            <Trash2 size={14} strokeWidth={1.8} />
            <span>移除本地项目</span>
          </button>
        )}
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
            disabled={saving || !name.trim() || roots.length === 0}
          >
            {saving
              ? isCreating
                ? "创建中…"
                : "保存中…"
              : isCreating
                ? "创建"
                : "保存"}
          </button>
        </div>
      </div>
    </ModalShell>
  );
}
