import {
  useRef,
  type CSSProperties,
  type HTMLAttributes,
  type ReactNode,
  type RefObject,
} from "react";
import { createPortal } from "react-dom";
import { useAnchoredMenu } from "../../hooks/ui/useAnchoredMenu";
import { useOverlayFocus } from "../../hooks/ui/useOverlayFocus";

type OverlayAccessibleName =
  | { "aria-label": string; "aria-labelledby"?: string }
  | { "aria-label"?: string; "aria-labelledby": string };

type OverlayCommonProps = Omit<
  HTMLAttributes<HTMLDivElement>,
  "children" | "aria-label" | "aria-labelledby"
> & OverlayAccessibleName & {
  open: boolean;
  onClose: () => void;
  children: ReactNode;
  initialFocusRef?: RefObject<HTMLElement>;
  backdropClassName?: string;
  backdropStyle?: CSSProperties;
  closeOnBackdrop?: boolean;
  closeOnEscape?: boolean;
  trapFocus?: boolean;
};

type OverlayFrameProps = OverlayCommonProps & {
  kind: "modal" | "drawer" | "popover";
  panelRef?: RefObject<HTMLDivElement>;
  panelStyle?: CSSProperties;
};

function OverlayFrame({
  open,
  onClose,
  children,
  initialFocusRef,
  closeOnBackdrop = true,
  closeOnEscape = true,
  trapFocus = true,
  kind,
  panelRef: providedPanelRef,
  panelStyle,
  backdropClassName = "",
  backdropStyle,
  className = "",
  role = "dialog",
  style,
  ...props
}: OverlayFrameProps) {
  const localPanelRef = useRef<HTMLDivElement>(null);
  const panelRef = providedPanelRef ?? localPanelRef;

  useOverlayFocus({
    open,
    onClose,
    containerRef: panelRef,
    initialFocusRef,
    closeOnEscape,
    trapFocus,
  });

  if (!open || typeof document === "undefined") return null;

  return createPortal(
    <div
      className={[
        "ui-overlay",
        `ui-overlay--${kind}`,
        backdropClassName,
      ]
        .filter(Boolean)
        .join(" ")}
      role="presentation"
      style={backdropStyle}
      onMouseDown={(event) => {
        if (closeOnBackdrop && event.target === event.currentTarget) onClose();
      }}
    >
      <div
        {...props}
        ref={panelRef}
        className={[
          "ui-overlay__surface",
          `ui-overlay__surface--${kind}`,
          className,
        ]
          .filter(Boolean)
          .join(" ")}
        style={{ ...style, ...panelStyle }}
        role={role}
        aria-modal={kind === "popover" ? undefined : true}
        tabIndex={-1}
        onMouseDown={(event) => {
          props.onMouseDown?.(event);
          event.stopPropagation();
        }}
      >
        {children}
      </div>
    </div>,
    document.body,
  );
}

export type ModalShellProps = OverlayCommonProps & {
  size?: "sm" | "md" | "lg";
};

export function ModalShell({
  size = "md",
  className = "",
  ...props
}: ModalShellProps) {
  return (
    <OverlayFrame
      {...props}
      kind="modal"
      className={`ui-modal-shell ui-modal-shell--${size} ${className}`.trim()}
    />
  );
}

export type DrawerProps = OverlayCommonProps & {
  side?: "start" | "end";
  size?: "sm" | "md" | "lg";
};

export function Drawer({
  side = "end",
  size = "md",
  className = "",
  ...props
}: DrawerProps) {
  return (
    <OverlayFrame
      {...props}
      kind="drawer"
      backdropClassName={`ui-overlay--drawer-${side}`}
      className={`ui-drawer ui-drawer--${size} ${className}`.trim()}
    />
  );
}

export type PopoverSurfaceProps = OverlayCommonProps & {
  anchorRef: RefObject<HTMLElement>;
  placement?: "auto" | "above" | "below";
  align?: "start" | "end";
  minWidth?: number;
  maxWidth?: number;
};

export function PopoverSurface({
  anchorRef,
  placement = "auto",
  align = "start",
  minWidth = 180,
  maxWidth = 320,
  className = "",
  open,
  trapFocus = false,
  ...props
}: PopoverSurfaceProps) {
  const panelRef = useRef<HTMLDivElement>(null);
  const position = useAnchoredMenu({
    open,
    anchorRef,
    menuRef: panelRef,
    minWidth,
    maxWidth,
    preferAlign: align,
    placement,
    maxHeightRatio: 0.65,
  });

  return (
    <OverlayFrame
      {...props}
      open={open}
      trapFocus={trapFocus}
      kind="popover"
      panelRef={panelRef}
      backdropClassName="ui-overlay--transparent"
      className={`ui-popover-surface${position?.openUp ? " is-up" : ""} ${className}`.trim()}
      panelStyle={{
        top: position?.top,
        left: position?.left,
        width: position?.width,
        maxWidth: position?.widthCap,
        maxHeight: position?.maxHeight,
        visibility: position ? "visible" : "hidden",
      }}
    />
  );
}
