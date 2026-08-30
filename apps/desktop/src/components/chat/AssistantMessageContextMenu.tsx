import {
  Check,
  ClipboardCopy,
  Copy,
  FileText,
  GitBranch,
  Layers3,
  List,
  Maximize2,
  Minimize2,
  RefreshCw,
  Settings2,
  type LucideIcon,
} from "lucide-react";
import { useEffect, useLayoutEffect, useRef, type KeyboardEvent } from "react";
import { createPortal } from "react-dom";
import type { ChatAnswerLayout } from "../../hooks/chat/useChatDisplayPrefs";
import { useI18n } from "../../i18n/LocaleContext";
import {
  clampPopover,
  measurePopoverSize,
  pointAnchor,
  resolveClipBoundsAt,
} from "../../lib/ui/clampPopover";

export type AssistantMessageMenuAction =
  | "layout-default"
  | "layout-timeline"
  | "layout-grouped"
  | "set-layout-default"
  | "toggle-process"
  | "copy-answer"
  | "copy-markdown"
  | "copy-process"
  | "regenerate"
  | "branch";

type Props = {
  x: number;
  y: number;
  defaultLayout: ChatAnswerLayout;
  layoutOverride?: ChatAnswerLayout;
  processExpanded: boolean;
  hasAnswer: boolean;
  hasProcess: boolean;
  canSetDefault: boolean;
  canRegenerate: boolean;
  canBranch: boolean;
  onAction: (action: AssistantMessageMenuAction) => void;
  onClose: (restoreFocus?: boolean) => void;
};

type ActionItem = {
  action: AssistantMessageMenuAction;
  label: string;
  Icon: LucideIcon;
  disabled?: boolean;
  checked?: boolean;
  separatorBefore?: boolean;
};

export default function AssistantMessageContextMenu({
  x,
  y,
  defaultLayout,
  layoutOverride,
  processExpanded,
  hasAnswer,
  hasProcess,
  canSetDefault,
  canRegenerate,
  canBranch,
  onAction,
  onClose,
}: Props) {
  const { t } = useI18n();
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onPointerDown = (event: MouseEvent) => {
      if (!ref.current?.contains(event.target as Node)) onClose(false);
    };
    const onKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape") onClose(true);
    };
    const onViewportChange = () => onClose(false);
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    window.addEventListener("resize", onViewportChange);
    window.addEventListener("blur", onViewportChange);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("resize", onViewportChange);
      window.removeEventListener("blur", onViewportChange);
    };
  }, [onClose]);

  useLayoutEffect(() => {
    const menu = ref.current;
    if (!menu) return;
    const position = clampPopover({
      anchorRect: pointAnchor(x, y),
      popoverSize: measurePopoverSize(menu),
      bounds: resolveClipBoundsAt(x, y),
      preferAlign: "start",
      placement: "auto",
      gap: 2,
      pad: 8,
    });
    menu.style.left = `${position.left}px`;
    menu.style.top = `${position.top}px`;
    menu.style.maxHeight = `${position.maxHeight}px`;
    const frame = window.requestAnimationFrame(() => {
      menu.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
    });
    return () => window.cancelAnimationFrame(frame);
  }, [x, y]);

  const activeLayout = layoutOverride ?? defaultLayout;
  const layoutItems: ActionItem[] = [
    {
      action: "layout-default",
      label: t("chat.messageMenu.followDefault"),
      Icon: Settings2,
      checked: layoutOverride == null,
    },
    {
      action: "layout-timeline",
      label: t("chat.messageMenu.timeline"),
      Icon: List,
      checked: layoutOverride === "timeline",
    },
    {
      action: "layout-grouped",
      label: t("chat.messageMenu.grouped"),
      Icon: Layers3,
      checked: layoutOverride === "grouped",
    },
  ];
  const actions: ActionItem[] = [
    {
      action: "set-layout-default",
      label: t("chat.messageMenu.setDefault", {
        layout: t(
          activeLayout === "grouped"
            ? "chat.messageMenu.grouped"
            : "chat.messageMenu.timeline",
        ),
      }),
      Icon: Settings2,
      disabled: !canSetDefault || layoutOverride == null,
      separatorBefore: true,
    },
    {
      action: "toggle-process",
      label: t(
        processExpanded
          ? "chat.messageMenu.collapseProcess"
          : "chat.messageMenu.expandProcess",
      ),
      Icon: processExpanded ? Minimize2 : Maximize2,
      disabled: !hasProcess,
    },
    {
      action: "copy-answer",
      label: t("chat.messageMenu.copyAnswer"),
      Icon: Copy,
      disabled: !hasAnswer,
      separatorBefore: true,
    },
    {
      action: "copy-markdown",
      label: t("chat.messageMenu.copyMarkdown"),
      Icon: FileText,
      disabled: !hasAnswer,
    },
    {
      action: "copy-process",
      label: t("chat.messageMenu.copyProcess"),
      Icon: ClipboardCopy,
      disabled: !hasProcess,
    },
    {
      action: "regenerate",
      label: t("chat.regenerate"),
      Icon: RefreshCw,
      disabled: !canRegenerate,
      separatorBefore: true,
    },
    {
      action: "branch",
      label: t("chat.branches.branchHere"),
      Icon: GitBranch,
      disabled: !canBranch,
    },
  ];

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
    event.preventDefault();
    const buttons = Array.from(
      ref.current?.querySelectorAll<HTMLButtonElement>(
        "button:not(:disabled)",
      ) ?? [],
    );
    if (buttons.length === 0) return;
    const current = buttons.indexOf(
      document.activeElement as HTMLButtonElement,
    );
    const next =
      event.key === "Home"
        ? 0
        : event.key === "End"
          ? buttons.length - 1
          : event.key === "ArrowDown"
            ? (current + 1 + buttons.length) % buttons.length
            : (current - 1 + buttons.length) % buttons.length;
    buttons[next]?.focus();
  };

  if (typeof document === "undefined") return null;
  return createPortal(
    <div
      ref={ref}
      className="assistant-message-menu"
      role="menu"
      aria-label={t("chat.messageMenu.title")}
      data-allow-context-menu
      style={{ left: x, top: y }}
      onKeyDown={handleKeyDown}
    >
      <div className="assistant-message-menu-label" role="presentation">
        {t("chat.messageMenu.display")}
      </div>
      {layoutItems.map(({ action, label, Icon, checked }) => (
        <button
          key={action}
          type="button"
          role="menuitemradio"
          aria-checked={checked}
          className="assistant-message-menu-item"
          onClick={() => {
            onAction(action);
            onClose(true);
          }}
        >
          <Icon size={15} strokeWidth={1.9} aria-hidden />
          <span>{label}</span>
          <Check
            className={`assistant-message-menu-check${checked ? " is-visible" : ""}`}
            size={14}
            strokeWidth={2.3}
            aria-hidden
          />
        </button>
      ))}
      {actions.map(({ action, label, Icon, disabled, separatorBefore }) => (
        <div
          key={action}
          role="presentation"
          className={
            separatorBefore ? "assistant-message-menu-group" : undefined
          }
        >
          {separatorBefore ? (
            <div
              className="assistant-message-menu-separator"
              role="separator"
            />
          ) : null}
          <button
            type="button"
            role="menuitem"
            className="assistant-message-menu-item"
            disabled={disabled}
            onClick={() => {
              if (disabled) return;
              onAction(action);
              onClose(true);
            }}
          >
            <Icon size={15} strokeWidth={1.9} aria-hidden />
            <span>{label}</span>
          </button>
        </div>
      ))}
    </div>,
    document.body,
  );
}
