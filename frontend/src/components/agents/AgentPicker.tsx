/** Agent 切换器。 */
import {
  useEffect,
  useId,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type SVGProps,
} from "react";
import { createPortal } from "react-dom";
import { useAnchoredMenu } from "../../hooks/ui/useAnchoredMenu";
import { useI18n } from "../../i18n/LocaleContext";
import type { MessageKey } from "../../i18n/messages";
import type { AgentInfo } from "../../types/agent";
import AgentAvatar from "./AgentAvatar";

/** Agent 下拉切换器入参 */
type Props = {
  agents: AgentInfo[];
  /** 当前选中 Agent id */
  value: string;
  onChange: (agentId: string) => void;
  disabled?: boolean;
  /** 无障碍 / 菜单标题 */
  labelKey?: MessageKey;
  className?: string;
  /** 菜单末项「新建 Agent」；提供时渲染分隔线 + 操作项 */
  onCreateNew?: () => void;
  /** 新建项文案；默认 chat.newAgent */
  createLabelKey?: MessageKey;
};

function agentSubline(
  agent: AgentInfo,
  defaultLabel: string,
): string | null {
  if (agent.vibe?.trim()) return agent.vibe.trim();
  if (agent.is_default) return defaultLabel;
  return null;
}

function ChevronDown(props: SVGProps<SVGSVGElement>) {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.25"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
      {...props}
    >
      <path d="m6 9 6 6 6-6" />
    </svg>
  );
}

export default function AgentPicker({
  agents,
  value,
  onChange,
  disabled = false,
  labelKey = "filespace.agentFilter",
  className = "",
  onCreateNew,
  createLabelKey = "chat.newAgent",
}: Props) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const [highlightedIndex, setHighlightedIndex] = useState(-1);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const menuRef = useRef<HTMLUListElement | null>(null);
  const listId = useId();
  const active = agents.find((a) => a.id === value) ?? agents[0];
  const defaultLabel = t("workspace.defaultAgent");
  const itemCount = agents.length + (onCreateNew ? 1 : 0);
  const createIndex = onCreateNew ? agents.length : -1;

  const pos = useAnchoredMenu({
    open,
    anchorRef: triggerRef,
    menuRef,
    sizeKey: `${agents.length}:${value}:${onCreateNew ? 1 : 0}`,
    minWidth: 220,
    maxWidth: 340,
    maxHeightCap: 300,
    maxHeightRatio: 0.42,
    preferAlign: "start",
    placement: "auto",
  });

  const optionId = (i: number) => `${listId}-opt-${i}`;

  const openMenu = (initialIndex?: number) => {
    const idx =
      initialIndex ?? agents.findIndex((a) => a.id === value);
    setHighlightedIndex(idx >= 0 ? idx : 0);
    setOpen(true);
  };

  const closeMenu = (restoreFocus = false) => {
    setOpen(false);
    setHighlightedIndex(-1);
    if (restoreFocus) triggerRef.current?.focus();
  };

  const activateHighlighted = () => {
    if (highlightedIndex < 0 || highlightedIndex >= itemCount) return;
    if (highlightedIndex === createIndex) {
      closeMenu(true);
      onCreateNew?.();
      return;
    }
    const agent = agents[highlightedIndex];
    if (!agent) return;
    closeMenu(true);
    if (agent.id !== value) onChange(agent.id);
  };

  const handleKeyDown = (e: KeyboardEvent<HTMLButtonElement>) => {
    if (disabled || agents.length === 0) return;
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        if (!open) openMenu();
        else
          setHighlightedIndex((i) =>
            Math.min(Math.max(i, 0) + 1, itemCount - 1),
          );
        break;
      case "ArrowUp":
        e.preventDefault();
        if (!open) openMenu(itemCount - 1);
        else setHighlightedIndex((i) => Math.max(i - 1, 0));
        break;
      case "Home":
        e.preventDefault();
        if (open) setHighlightedIndex(0);
        break;
      case "End":
        e.preventDefault();
        if (open) setHighlightedIndex(itemCount - 1);
        break;
      case "Enter":
      case " ":
        e.preventDefault();
        if (!open) openMenu();
        else activateHighlighted();
        break;
      case "Escape":
        if (open) {
          e.preventDefault();
          closeMenu(true);
        }
        break;
      default:
        break;
    }
  };

  useEffect(() => {
    if (!open || highlightedIndex < 0 || !menuRef.current) return;
    const items = menuRef.current.querySelectorAll<HTMLElement>("[data-menu-index]");
    items[highlightedIndex]?.scrollIntoView({ block: "nearest" });
  }, [open, highlightedIndex]);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (e: MouseEvent) => {
      const target = e.target as Node;
      if (rootRef.current?.contains(target)) return;
      if (menuRef.current?.contains(target)) return;
      closeMenu();
    };
    document.addEventListener("mousedown", onPointerDown);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
    };
  }, [open]);

  // Portal 丢 CSS 变量：从触发器节点带 tone / 毛玻璃令牌到浮层
  const menuToneStyle =
    open && rootRef.current
      ? (() => {
          const cs = getComputedStyle(rootRef.current!);
          const pick = (name: string) => cs.getPropertyValue(name).trim();
          const tone = pick("--tone");
          const soft = pick("--tone-soft");
          const bg = pick("--menu-glass-bg");
          const border = pick("--menu-glass-border");
          const shadow = pick("--menu-glass-shadow");
          const blur = pick("--menu-glass-blur");
          return {
            ...(tone ? { ["--tone"]: tone } : {}),
            ...(soft ? { ["--tone-soft"]: soft } : {}),
            ...(bg ? { ["--menu-glass-bg"]: bg } : {}),
            ...(border ? { ["--menu-glass-border"]: border } : {}),
            ...(shadow ? { ["--menu-glass-shadow"]: shadow } : {}),
            ...(blur ? { ["--menu-glass-blur"]: blur } : {}),
          } as CSSProperties;
        })()
      : undefined;

  if (agents.length === 0) {
    return (
      <div className={`agent-picker ${className}`.trim()} ref={rootRef}>
        <button
          type="button"
          className="agent-picker-chip"
          disabled
          aria-label={t(labelKey)}
        >
          <span className="agent-picker-meta">
            <span className="agent-picker-name">{t(labelKey)}</span>
          </span>
          <ChevronDown className="agent-picker-chevron" />
        </button>
      </div>
    );
  }

  const activeSub = active ? agentSubline(active, defaultLabel) : null;

  const menu =
    open && typeof document !== "undefined"
      ? createPortal(
          <ul
            ref={menuRef}
            id={listId}
            className={`agent-picker-menu${pos?.openUp ? " is-up" : ""}`}
            role="listbox"
            aria-label={t(labelKey)}
            style={{
              ...menuToneStyle,
              ...(pos
                ? {
                    top: pos.top,
                    left: pos.left,
                    width: pos.width,
                    maxHeight: pos.maxHeight,
                  }
                : { visibility: "hidden" as const }),
            }}
          >
            {agents.map((a, i) => {
              const sub = agentSubline(a, defaultLabel);
              const highlighted = i === highlightedIndex;
              return (
                <li
                  key={a.id}
                  id={optionId(i)}
                  role="option"
                  aria-selected={a.id === value}
                  data-menu-index={i}
                >
                  <button
                    type="button"
                    tabIndex={-1}
                    className={`agent-picker-option ${a.id === value ? "is-active" : ""}${highlighted ? " is-highlighted" : ""}`}
                    onMouseEnter={() => setHighlightedIndex(i)}
                    onClick={() => {
                      closeMenu(true);
                      if (a.id !== value) onChange(a.id);
                    }}
                  >
                    <AgentAvatar agent={a} size={22} />
                    <span className="agent-picker-option-text">
                      <span className="agent-picker-option-name">{a.name}</span>
                      {sub ? (
                        <span className="agent-picker-option-sub">{sub}</span>
                      ) : null}
                    </span>
                  </button>
                </li>
              );
            })}
            {onCreateNew ? (
              <li
                id={optionId(createIndex)}
                role="option"
                className="agent-picker-create"
                aria-selected={false}
                data-menu-index={createIndex}
              >
                <button
                  type="button"
                  tabIndex={-1}
                  className={`agent-picker-option agent-picker-option--create${
                    highlightedIndex === createIndex ? " is-highlighted" : ""
                  }`}
                  onMouseEnter={() => setHighlightedIndex(createIndex)}
                  onClick={() => {
                    closeMenu(true);
                    onCreateNew();
                  }}
                >
                  <span className="agent-picker-create-icon" aria-hidden>
                    +
                  </span>
                  <span className="agent-picker-option-text">
                    <span className="agent-picker-option-name">
                      {t(createLabelKey)}
                    </span>
                  </span>
                </button>
              </li>
            ) : null}
          </ul>,
          document.body,
        )
      : null;

  return (
    <div className={`agent-picker ${className}`.trim()} ref={rootRef}>
      <button
        ref={triggerRef}
        type="button"
        className={`agent-picker-chip ${open ? "is-open" : ""}`}
        disabled={disabled}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={open ? listId : undefined}
        aria-activedescendant={
          open && highlightedIndex >= 0 ? optionId(highlightedIndex) : undefined
        }
        aria-label={t(labelKey)}
        onClick={() => {
          if (disabled) return;
          if (open) closeMenu();
          else openMenu();
        }}
        onKeyDown={handleKeyDown}
      >
        {active ? (
          <>
            <AgentAvatar agent={active} size={24} />
            <span className="agent-picker-meta">
              <span className="agent-picker-name">{active.name}</span>
              {activeSub ? (
                <span className="agent-picker-vibe">{activeSub}</span>
              ) : null}
            </span>
          </>
        ) : (
          <span className="agent-picker-meta">
            <span className="agent-picker-name">—</span>
          </span>
        )}
        <ChevronDown className="agent-picker-chevron" />
      </button>
      {menu}
    </div>
  );
}
