// 项目右键菜单 / 更多菜单。
import { useEffect, useRef } from "react";
import {
  Edit3,
  FolderOpen,
  GitBranch,
  Archive,
  Pin,
  X,
} from "lucide-react";

export type ProjectMenuAction =
  | "pin"
  | "edit"
  | "reveal"
  | "worktree"
  | "archive"
  | "remove";

type Props = {
  x: number;
  y: number;
  projectName: string;
  projectPath: string;
  onAction: (action: ProjectMenuAction) => void;
  onClose: () => void;
};

const ITEMS: Array<{
  id: ProjectMenuAction;
  label: string;
  Icon: typeof Pin;
  separator?: boolean;
}> = [
  { id: "pin", label: "置顶", Icon: Pin },
  { id: "edit", label: "编辑", Icon: Edit3 },
  { id: "reveal", label: "在 Finder 中显示", Icon: FolderOpen, separator: true },
  { id: "worktree", label: "创建永久工作树", Icon: GitBranch },
  { id: "archive", label: "归档聊天", Icon: Archive, separator: true },
  { id: "remove", label: "移除项目", Icon: X },
];

export default function ProjectContextMenu({
  x,
  y,
  projectName: _projectName,
  projectPath: _projectPath,
  onAction,
  onClose,
}: Props) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onClick = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("mousedown", onClick);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onClick);
      window.removeEventListener("keydown", onKey);
    };
  }, [onClose]);

  return (
    <div
      ref={ref}
      className="project-context-menu"
      style={{ top: y, left: x }}
      role="menu"
    >
      {ITEMS.map((item) => (
        <button
          key={item.id}
          type="button"
          role="menuitem"
          className={`project-context-menu-item ${item.id === "remove" ? "is-danger" : ""}`}
          onClick={() => { onAction(item.id); onClose(); }}
        >
          <item.Icon size={14} strokeWidth={1.8} aria-hidden />
          <span>{item.label}</span>
        </button>
      ))}
    </div>
  );
}
