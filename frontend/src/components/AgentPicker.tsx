/** Agent 切换器。 */
import {
  useEffect,
  useId,
  useRef,
  useState,
  type CSSProperties,
  type SVGProps,
} from "react";
import { createPortal } from "react-dom";
import { useAnchoredMenu } from "../hooks/useAnchoredMenu";
import { useI18n } from "../i18n/LocaleContext";
import type { MessageKey } from "../i18n/messages";
import type { AgentInfo } from "../types/agent";
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
  const rootRef = useRef<HTMLDivElement | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const menuRef = useRef<HTMLUListElement | null>(null);
  const listId = useId();
  const active = agents.find((a) => a.id === value) ?? agents[0];
  const defaultLabel = t("workspace.defaultAgent");

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

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (e: MouseEvent) => {
      const target = e.target as Node;
      if (rootRef.current?.contains(target)) return;
      if (menuRef.current?.contains(target)) return;
      setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKey);
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
            {agents.map((a) => {
              const sub = agentSubline(a, defaultLabel);
              return (
                <li key={a.id} role="option" aria-selected={a.id === value}>
                  <button
                    type="button"
                    className={`agent-picker-option ${a.id === value ? "is-active" : ""}`}
                    onClick={() => {
                      setOpen(false);
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
                role="option"
                className="agent-picker-create"
                aria-selected={false}
              >
                <button
                  type="button"
                  className="agent-picker-option agent-picker-option--create"
                  onClick={() => {
                    setOpen(false);
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
        aria-label={t(labelKey)}
        onClick={() => setOpen((v) => !v)}
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
