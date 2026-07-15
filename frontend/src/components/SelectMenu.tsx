/** 通用下拉选择菜单。 */
import {
  useEffect,
  useId,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import { useAnchoredMenu } from "../hooks/useAnchoredMenu";

/** 下拉选项 */
export type SelectOption = {
  value: string;
  label: string;
  /** 可选前缀图标（如模型/供应商品牌） */
  icon?: ReactNode;
};

/** 通用下拉选择菜单入参 */
type Props = {
  value: string;
  options: SelectOption[];
  onChange: (value: string) => void;
  disabled?: boolean;
  className?: string;
  /** 紧凑尺寸（如间隔单位） */
  size?: "md" | "sm";
  "aria-label"?: string;
  placeholder?: string;
  /** 展开方向：auto（默认）/ up（强制向上）/ down（强制向下） */
  openDirection?: "auto" | "up" | "down";
};

const VIEWPORT_PAD = 8;

/** 触发器右侧展开箭头 */
function ChevronDown({ open }: { open: boolean }) {
  return (
    <svg
      className={`select-menu-chevron${open ? " is-open" : ""}`}
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.25"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <path d="m6 9 6 6 6-6" />
    </svg>
  );
}

/** 选中项勾选标记 */
function CheckIcon() {
  return (
    <svg
      className="select-menu-check"
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <path d="M20 6 9 17l-5-5" />
    </svg>
  );
}

/** 通用下拉选择菜单（Portal 列表） */
export function SelectMenu({
  value,
  options,
  onChange,
  disabled = false,
  className = "",
  size = "md",
  "aria-label": ariaLabel,
  placeholder = "—",
  openDirection = "auto",
}: Props) {
  const [open, setOpen] = useState(false);
  const [highlightedIndex, setHighlightedIndex] = useState(-1);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const listRef = useRef<HTMLUListElement | null>(null);
  const listId = useId();
  const selected = options.find((o) => o.value === value);
  const label = selected?.label ?? placeholder;

  const placement =
    openDirection === "up"
      ? "above"
      : openDirection === "down"
        ? "below"
        : "auto";

  const pos = useAnchoredMenu({
    open,
    anchorRef: triggerRef,
    menuRef: listRef,
    sizeKey: `${options.length}:${value}:${openDirection}`,
    minWidth: 140,
    maxWidth: 280,
    maxHeightCap: 260,
    maxHeightRatio: 0.42,
    preferAlign: "start",
    placement,
  });

  const optionId = (i: number) => `${listId}-opt-${i}`;

  const openMenu = (initialIndex?: number) => {
    const idx = initialIndex ?? options.findIndex((o) => o.value === value);
    setHighlightedIndex(idx >= 0 ? idx : 0);
    setOpen(true);
  };

  const closeMenu = () => {
    setOpen(false);
    setHighlightedIndex(-1);
  };

  const selectHighlighted = () => {
    if (highlightedIndex >= 0 && highlightedIndex < options.length) {
      const opt = options[highlightedIndex]!;
      closeMenu();
      if (opt.value !== value) onChange(opt.value);
    }
  };

  const handleKeyDown = (e: KeyboardEvent<HTMLButtonElement>) => {
    if (disabled) return;
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        if (!open) {
          openMenu();
        } else {
          setHighlightedIndex((i) => Math.min(i + 1, options.length - 1));
        }
        break;
      case "ArrowUp":
        e.preventDefault();
        if (!open) {
          openMenu(options.length - 1);
        } else {
          setHighlightedIndex((i) => Math.max(i - 1, 0));
        }
        break;
      case "Home":
        e.preventDefault();
        if (open) setHighlightedIndex(0);
        break;
      case "End":
        e.preventDefault();
        if (open) setHighlightedIndex(options.length - 1);
        break;
      case "Enter":
      case " ":
        e.preventDefault();
        if (!open) {
          openMenu();
        } else {
          selectHighlighted();
        }
        break;
      case "Escape":
        if (open) {
          e.preventDefault();
          closeMenu();
          triggerRef.current?.focus();
        }
        break;
      default:
        if (open && e.key.length === 1) {
          const ch = e.key.toLowerCase();
          const start = highlightedIndex >= 0 ? highlightedIndex + 1 : 0;
          const rotated = [...options.slice(start), ...options.slice(0, start)];
          const idx = rotated.findIndex((o) =>
            o.label.toLowerCase().startsWith(ch),
          );
          if (idx >= 0) {
            setHighlightedIndex((start + idx) % options.length);
          }
        }
    }
  };

  useEffect(() => {
    if (!open || highlightedIndex < 0 || !listRef.current) return;
    const items = listRef.current.querySelectorAll<HTMLElement>('[role="option"]');
    items[highlightedIndex]?.scrollIntoView({ block: "nearest" });
  }, [open, highlightedIndex]);

  useEffect(() => {
    if (!open) return;
    const onMouseDown = (e: MouseEvent) => {
      const target = e.target as Node;
      if (rootRef.current?.contains(target)) return;
      if (listRef.current?.contains(target)) return;
      closeMenu();
    };
    document.addEventListener("mousedown", onMouseDown);
    return () => {
      document.removeEventListener("mousedown", onMouseDown);
    };
  }, [open]);

  const menuToneStyle =
    open && rootRef.current
      ? (() => {
          const cs = getComputedStyle(rootRef.current!);
          const tone = cs.getPropertyValue("--select-tone").trim();
          const soft = cs.getPropertyValue("--select-tone-soft").trim();
          return {
            ...(tone ? { ["--select-tone"]: tone } : {}),
            ...(soft ? { ["--select-tone-soft"]: soft } : {}),
          } as CSSProperties;
        })()
      : undefined;

  const menu =
    open && pos
      ? createPortal(
          <ul
            id={listId}
            ref={listRef}
            className={`select-menu-list${pos.openUp ? " is-up" : ""}`}
            role="listbox"
            aria-label={ariaLabel}
            style={{
              top: pos.top,
              left: pos.left,
              width: "max-content",
              minWidth: Math.max(pos.width, 140),
              maxWidth: `min(280px, calc(100vw - ${VIEWPORT_PAD * 2}px))`,
              maxHeight: pos.maxHeight,
              ...menuToneStyle,
            }}
          >
            {options.map((opt, i) => {
              const active = opt.value === value;
              const highlighted = i === highlightedIndex;
              return (
                <li
                  key={opt.value}
                  id={optionId(i)}
                  role="option"
                  aria-selected={active}
                >
                  <button
                    type="button"
                    className={`select-menu-option${active ? " is-active" : ""}${highlighted ? " is-highlighted" : ""}`}
                    tabIndex={-1}
                    onMouseEnter={() => setHighlightedIndex(i)}
                    onClick={() => {
                      closeMenu();
                      if (opt.value !== value) onChange(opt.value);
                      triggerRef.current?.focus();
                    }}
                  >
                    {opt.icon ? (
                      <span className="select-menu-option-icon" aria-hidden>
                        {opt.icon}
                      </span>
                    ) : null}
                    <span className="select-menu-option-label">{opt.label}</span>
                    {active ? <CheckIcon /> : null}
                  </button>
                </li>
              );
            })}
          </ul>,
          document.body,
        )
      : null;

  return (
    <div
      ref={rootRef}
      className={`select-menu select-menu--${size} ${open ? "is-open" : ""} ${className}`.trim()}
    >
      <button
        ref={triggerRef}
        type="button"
        className="select-menu-trigger"
        disabled={disabled}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={open ? listId : undefined}
        aria-activedescendant={
          open && highlightedIndex >= 0 ? optionId(highlightedIndex) : undefined
        }
        aria-label={ariaLabel}
        onClick={() => {
          if (!disabled) {
            if (open) closeMenu();
            else openMenu();
          }
        }}
        onKeyDown={handleKeyDown}
      >
        {selected?.icon ? (
          <span className="select-menu-value-icon" aria-hidden>
            {selected.icon}
          </span>
        ) : null}
        <span className="select-menu-value">{label}</span>
        <ChevronDown open={open} />
      </button>
      {menu}
    </div>
  );
}
