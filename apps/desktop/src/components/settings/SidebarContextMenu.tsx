/** 侧栏右键菜单：显示/隐藏名称、展开/收起导航栏。 */
import { useEffect, useLayoutEffect, useRef, type ComponentType } from "react";
import { createPortal } from "react-dom";
import { PanelLeftClose, PanelLeftOpen, Type, type LucideProps } from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";
import type { MessageKey } from "../../i18n/messages";
import {
  clampPopover,
  measurePopoverSize,
  pointAnchor,
  resolveClipBoundsAt,
} from "../../lib/ui/clampPopover";

export type SidebarMenuAction = "toggleLabels" | "togglePin";

type Item = {
  action: SidebarMenuAction;
  labelKey: MessageKey;
  Icon: ComponentType<LucideProps>;
};

type Props = {
  x: number;
  y: number;
  labelsVisible: boolean;
  pinned: boolean;
  onAction: (action: SidebarMenuAction) => void;
  onClose: () => void;
};

const ICO = { size: 14, strokeWidth: 2.1 } as const;

export default function SidebarContextMenu({
  x,
  y,
  labelsVisible,
  pinned,
  onAction,
  onClose,
}: Props) {
  const { t } = useI18n();
  const ref = useRef<HTMLDivElement | null>(null);

  const items: Item[] = [
    {
      action: "toggleLabels",
      labelKey: labelsVisible ? "sidebar.menu.hideLabels" : "sidebar.menu.showLabels",
      Icon: Type,
    },
    {
      action: "togglePin",
      labelKey: pinned ? "sidebar.menu.collapse" : "sidebar.menu.expand",
      Icon: pinned ? PanelLeftClose : PanelLeftOpen,
    },
  ];

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
    const size = measurePopoverSize(el);
    const pos = clampPopover({
      anchorRect: pointAnchor(x, y),
      popoverSize: size,
      bounds: resolveClipBoundsAt(x, y),
      preferAlign: "start",
      placement: "auto",
      gap: 0,
      pad: 8,
    });
    el.style.left = `${pos.left}px`;
    el.style.top = `${pos.top}px`;
  }, [x, y, labelsVisible, pinned]);

  const menu = (
    <div
      className="fs-ctx-menu"
      ref={ref}
      role="menu"
      data-allow-context-menu
      style={{ left: x, top: y }}
    >
      {items.map((it) => (
        <button
          key={it.action}
          type="button"
          role="menuitem"
          className="fs-ctx-item"
          onClick={() => onAction(it.action)}
        >
          <it.Icon className="fs-ctx-ico" {...ICO} aria-hidden />
          <span>{t(it.labelKey)}</span>
        </button>
      ))}
    </div>
  );

  if (typeof document === "undefined") return null;
  return createPortal(menu, document.body);
}
