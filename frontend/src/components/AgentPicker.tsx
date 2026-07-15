/** Agent 切换器。 */
import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type SVGProps,
} from "react";
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

const VIEWPORT_PAD = 8;

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
  const [menuStyle, setMenuStyle] = useState<CSSProperties | undefined>();
  const ref = useRef<HTMLDivElement | null>(null);
  const menuRef = useRef<HTMLUListElement | null>(null);
  const active = agents.find((a) => a.id === value) ?? agents[0];
  const defaultLabel = t("workspace.defaultAgent");

  useLayoutEffect(() => {
    if (!open || !ref.current || !menuRef.current) {
      setMenuStyle(undefined);
      return;
    }
    const clamp = () => {
      const root = ref.current;
      const menu = menuRef.current;
      if (!root || !menu) return;
      const rootRect = root.getBoundingClientRect();
      const menuRect = menu.getBoundingClientRect();
      const maxW = Math.min(340, window.innerWidth - VIEWPORT_PAD * 2);
      let left = 0;
      // 相对 root：默认左对齐；右侧溢出视口则改为右对齐
      if (rootRect.left + Math.min(menuRect.width, maxW) > window.innerWidth - VIEWPORT_PAD) {
        left = rootRect.width - Math.min(menuRect.width, maxW);
      }
      const absLeft = rootRect.left + left;
      if (absLeft < VIEWPORT_PAD) {
        left += VIEWPORT_PAD - absLeft;
      }
      const absRight = rootRect.left + left + Math.min(menuRect.width, maxW);
      if (absRight > window.innerWidth - VIEWPORT_PAD) {
        left -= absRight - (window.innerWidth - VIEWPORT_PAD);
      }
      setMenuStyle({
        left,
        right: "auto",
        maxWidth: maxW,
      });
    };
    clamp();
    const raf = requestAnimationFrame(clamp);
    window.addEventListener("resize", clamp);
    window.addEventListener("scroll", clamp, true);
    return () => {
      cancelAnimationFrame(raf);
      window.removeEventListener("resize", clamp);
      window.removeEventListener("scroll", clamp, true);
    };
  }, [open, agents, value]);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
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

  if (agents.length === 0) {
    return (
      <div className={`agent-picker ${className}`.trim()}>
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

  return (
    <div className={`agent-picker ${className}`.trim()} ref={ref}>
      <button
        type="button"
        className={`agent-picker-chip ${open ? "is-open" : ""}`}
        disabled={disabled}
        aria-haspopup="listbox"
        aria-expanded={open}
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
      {open ? (
        <ul
          ref={menuRef}
          className="agent-picker-menu"
          role="listbox"
          aria-label={t(labelKey)}
          style={menuStyle}
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
        </ul>
      ) : null}
    </div>
  );
}
