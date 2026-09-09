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
import { Search } from "lucide-react";
import { createPortal } from "react-dom";
import {
  ChevronDown as ChevronDownData,
  ChevronUp as ChevronUpData,
} from "lucide";
import { useAnchoredMenu } from "../../hooks/ui/useAnchoredMenu";
import { useDynamicOverlayLayer } from "../../hooks/ui/useDynamicOverlayLayer";
import { MorphToggleIcon } from "../icons/MorphIcon";

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
  /** 选中项标记：默认勾选，单选筛选可使用圆点。 */
  selectionIndicator?: "check" | "radio";
  /** 长选项列表的可视高度上限。 */
  menuMaxHeight?: number;
  /** Opt-in filtering; labels are supplied by the calling surface for localization. */
  search?: { placeholder: string; emptyLabel: string };
  /** A new positive request opens the menu once after options become available. */
  openRequest?: number;
};

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

function RadioIcon() {
  return (
    <span className="select-menu-radio" aria-hidden>
      <span />
    </span>
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
  selectionIndicator = "check",
  menuMaxHeight = 260,
  search,
  openRequest = 0,
}: Props) {
  const [open, setOpen] = useState(false);
  const handledOpenRequest = useRef(0);
  const [query, setQuery] = useState("");
  const searchEnabled = Boolean(search);
  const normalizedQuery = query.trim().toLocaleLowerCase();
  const visibleOptions =
    searchEnabled && normalizedQuery
      ? options.filter((option) =>
          `${option.label} ${option.value}`
            .toLocaleLowerCase()
            .includes(normalizedQuery),
        )
      : options;
  const [highlightedIndex, setHighlightedIndex] = useState(-1);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const listRef = useRef<HTMLUListElement | null>(null);
  const popupRef = useRef<HTMLDivElement | null>(null);
  const searchRef = useRef<HTMLInputElement | null>(null);
  const listId = useId();
  const popupId = `${listId}-popup`;
  const { layer, bringToFront } = useDynamicOverlayLayer(open);
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
    menuRef: searchEnabled ? popupRef : listRef,
    sizeKey: `${visibleOptions.length}:${query}:${value}:${openDirection}:${menuMaxHeight}`,
    minWidth: 140,
    maxWidth: searchEnabled ? 360 : 280,
    maxHeightCap: menuMaxHeight,
    maxHeightRatio: menuMaxHeight > 260 ? 0.8 : 0.42,
    preferAlign: "start",
    placement,
  });

  const optionId = (i: number) => `${listId}-opt-${i}`;

  const openMenu = (initialIndex?: number) => {
    const idx = initialIndex ?? options.findIndex((o) => o.value === value);
    setHighlightedIndex(idx >= 0 ? idx : 0);
    setQuery("");
    setOpen(true);
  };

  useEffect(() => {
    if (!disabled && openRequest > handledOpenRequest.current) {
      handledOpenRequest.current = openRequest;
      openMenu();
    }
  }, [openRequest, disabled]);

  const closeMenu = (restoreFocus = false) => {
    setOpen(false);
    setQuery("");
    setHighlightedIndex(-1);
    if (restoreFocus) triggerRef.current?.focus();
  };

  const selectHighlighted = () => {
    if (highlightedIndex >= 0 && highlightedIndex < visibleOptions.length) {
      const opt = visibleOptions[highlightedIndex]!;
      closeMenu(true);
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
          closeMenu(true);
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

  const handleSearchKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.nativeEvent.isComposing || event.keyCode === 229) return;
    switch (event.key) {
      case "ArrowDown":
        event.preventDefault();
        setHighlightedIndex((i) => Math.min(i + 1, visibleOptions.length - 1));
        break;
      case "ArrowUp":
        event.preventDefault();
        setHighlightedIndex((i) =>
          visibleOptions.length ? Math.max(i - 1, 0) : -1,
        );
        break;
      case "Enter":
        event.preventDefault();
        selectHighlighted();
        break;
      case "Escape":
        event.preventDefault();
        event.stopPropagation();
        closeMenu(true);
        break;
      case "Tab":
        closeMenu(true);
        break;
    }
  };

  const positioned = Boolean(pos);
  useEffect(() => {
    if (open && searchEnabled && positioned) searchRef.current?.focus();
  }, [open, searchEnabled, positioned]);

  useEffect(() => {
    if (disabled && open) closeMenu();
  }, [disabled, open]);

  useEffect(() => {
    if (!open || highlightedIndex < 0 || !listRef.current) return;
    const items =
      listRef.current.querySelectorAll<HTMLElement>('[role="option"]');
    items[highlightedIndex]?.scrollIntoView({ block: "nearest" });
  }, [open, positioned, highlightedIndex, query]);

  useEffect(() => {
    if (!open) return;
    const onMouseDown = (e: MouseEvent) => {
      const target = e.target as Node;
      if (rootRef.current?.contains(target)) return;
      if (listRef.current?.contains(target)) return;
      if (popupRef.current?.contains(target)) return;
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

  const menuStyle: CSSProperties | undefined = pos
    ? {
        top: pos.top,
        left: pos.left,
        width: "max-content",
        minWidth: Math.max(pos.width, 140),
        maxWidth: pos.widthCap,
        maxHeight: pos.maxHeight,
        zIndex: layer,
        ...menuToneStyle,
      }
    : undefined;
  const menuClassName = `select-menu-list${pos?.openUp ? " is-up" : ""}`;
  const list =
    open && pos ? (
      <ul
        id={listId}
        ref={listRef}
        className={searchEnabled ? "select-menu-results" : menuClassName}
        role="listbox"
        aria-label={ariaLabel}
        style={searchEnabled ? undefined : menuStyle}
        onPointerDownCapture={searchEnabled ? undefined : bringToFront}
      >
        {visibleOptions.map((opt, i) => {
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
                title={searchEnabled ? opt.label : undefined}
                onMouseDown={(event) => {
                  if (searchEnabled) event.preventDefault();
                }}
                onMouseEnter={() => setHighlightedIndex(i)}
                onClick={() => {
                  closeMenu(true);
                  if (opt.value !== value) onChange(opt.value);
                }}
              >
                {opt.icon ? (
                  <span className="select-menu-option-icon" aria-hidden>
                    {opt.icon}
                  </span>
                ) : null}
                <span className="select-menu-option-label">{opt.label}</span>
                {active ? (
                  selectionIndicator === "radio" ? (
                    <RadioIcon />
                  ) : (
                    <CheckIcon />
                  )
                ) : selectionIndicator === "radio" ? (
                  <span className="select-menu-radio" aria-hidden />
                ) : null}
              </button>
            </li>
          );
        })}
      </ul>
    ) : null;
  const menu =
    open && pos
      ? createPortal(
          search ? (
            <div
              id={popupId}
              ref={popupRef}
              className={`${menuClassName} select-menu-list--searchable`}
              role="dialog"
              aria-label={ariaLabel}
              style={menuStyle}
              onPointerDownCapture={bringToFront}
            >
              <div className="select-menu-search">
                <Search size={15} aria-hidden="true" />
                <input
                  ref={searchRef}
                  type="text"
                  role="combobox"
                  aria-label={search.placeholder}
                  placeholder={search.placeholder}
                  aria-autocomplete="list"
                  aria-expanded={open}
                  aria-controls={listId}
                  aria-activedescendant={
                    highlightedIndex >= 0 &&
                    highlightedIndex < visibleOptions.length
                      ? optionId(highlightedIndex)
                      : undefined
                  }
                  autoComplete="off"
                  spellCheck={false}
                  value={query}
                  onChange={(event) => {
                    setQuery(event.target.value);
                    setHighlightedIndex(0);
                  }}
                  onKeyDown={handleSearchKeyDown}
                />
              </div>
              {list}
              {visibleOptions.length === 0 && (
                <p className="select-menu-empty" role="status">
                  {search.emptyLabel}
                </p>
              )}
            </div>
          ) : (
            list
          ),
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
        aria-haspopup={searchEnabled ? "dialog" : "listbox"}
        aria-expanded={open}
        aria-controls={open ? (searchEnabled ? popupId : listId) : undefined}
        aria-activedescendant={
          !searchEnabled && open && highlightedIndex >= 0
            ? optionId(highlightedIndex)
            : undefined
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
        <MorphToggleIcon
          className={`select-menu-chevron${open ? " is-open" : ""}`}
          active={open}
          activeIcon={ChevronUpData}
          inactiveIcon={ChevronDownData}
          size={14}
          strokeWidth={2.25}
          aria-hidden
        />
      </button>
      {menu}
    </div>
  );
}
