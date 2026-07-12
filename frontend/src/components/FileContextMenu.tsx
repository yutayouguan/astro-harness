/** 文件右键菜单（Portal）。 */
import { useEffect, useLayoutEffect, useRef, type ComponentType } from "react";
import { createPortal } from "react-dom";
import {
  ClipboardCopy,
  ClipboardPaste,
  Copy,
  ExternalLink,
  File,
  FilePlus,
  FileX,
  FolderOpen,
  FolderPlus,
  MessageSquare,
  MessageSquarePlus,
  Pencil,
  Scissors,
  Trash2,
  type LucideProps,
} from "lucide-react";
import { useI18n } from "../i18n/LocaleContext";
import type { MessageKey } from "../i18n/messages";

export type FileMenuAction =
  | "open"
  | "reveal"
  | "copyPath"
  | "copyFile"
  | "attachNew"
  | "attachCurrent"
  | "trash"
  | "removeIndex"
  | "rename"
  | "cut"
  | "paste"
  | "newFile"
  | "newFolder"
  | "openExternally";

/** 菜单单项 */
type Item = {
  action: FileMenuAction;
  labelKey: MessageKey;
  /** 覆盖 i18n 的自定义文案 */
  label?: string;
  disabled?: boolean;
  danger?: boolean;
  /** 该项上方渲染分隔线 */
  separatorBefore?: boolean;
};

/** 文件右键菜单入参（Portal 到 body） */
type Props = {
  /** 视口 X（`clientX`），fixed 定位 */
  x: number;
  /** 视口 Y（`clientY`） */
  y: number;
  items: Item[];
  onAction: (action: FileMenuAction) => void;
  onClose: () => void;
  /** 额外 class（如工作区玻璃变体 `ws-ctx-menu`） */
  className?: string;
};

const EDGE = 8;

const ICO = { size: 14, strokeWidth: 2.1 } as const;

const ACTION_ICONS: Record<FileMenuAction, ComponentType<LucideProps>> = {
  open: File,
  reveal: FolderOpen,
  copyPath: ClipboardCopy,
  copyFile: Copy,
  attachNew: MessageSquarePlus,
  attachCurrent: MessageSquare,
  trash: Trash2,
  removeIndex: FileX,
  rename: Pencil,
  cut: Scissors,
  paste: ClipboardPaste,
  newFile: FilePlus,
  newFolder: FolderPlus,
  openExternally: ExternalLink,
};

/** Clamp menu to viewport; flip to the other side of the cursor when near right/bottom. */
function clampMenuPosition(
  cursorX: number,
  cursorY: number,
  width: number,
  height: number,
  vw: number,
  vh: number,
) {
  let left = cursorX;
  let top = cursorY;

  if (left + width > vw - EDGE) {
    left = cursorX - width;
  }
  if (left < EDGE) {
    left = Math.max(EDGE, Math.min(cursorX, vw - width - EDGE));
  }

  if (top + height > vh - EDGE) {
    top = cursorY - height;
  }
  if (top < EDGE) {
    top = Math.max(EDGE, Math.min(cursorY, vh - height - EDGE));
  }

  return {
    left: Math.max(EDGE, Math.min(left, vw - width - EDGE)),
    top: Math.max(EDGE, Math.min(top, vh - height - EDGE)),
  };
}

export default function FileContextMenu({ x, y, items, onAction, onClose, className }: Props) {
  const { t } = useI18n();
  const ref = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [onClose]);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    const { left, top } = clampMenuPosition(
      x,
      y,
      rect.width,
      rect.height,
      window.innerWidth,
      window.innerHeight,
    );
    el.style.left = `${left}px`;
    el.style.top = `${top}px`;
  }, [x, y, items]);

  const menu = (
    <div
      className={["fs-ctx-menu", className].filter(Boolean).join(" ")}
      ref={ref}
      role="menu"
      style={{ left: x, top: y }}
    >
      {items.map((it) => {
        const Icon = ACTION_ICONS[it.action];
        return (
          <div key={it.action} className={it.separatorBefore ? "fs-ctx-group" : undefined}>
            {it.separatorBefore ? <div className="fs-ctx-sep" role="separator" /> : null}
            <button
              type="button"
              role="menuitem"
              className={`fs-ctx-item ${it.danger ? "is-danger" : ""}`}
              disabled={it.disabled}
              onClick={() => {
                if (!it.disabled) onAction(it.action);
              }}
            >
              <Icon className="fs-ctx-ico" {...ICO} aria-hidden />
              <span>{it.label ?? t(it.labelKey)}</span>
            </button>
          </div>
        );
      })}
    </div>
  );

  if (typeof document === "undefined") return null;
  return createPortal(menu, document.body);
}
