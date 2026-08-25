/** 可展开搜索框。 */
import { useEffect, useRef, useState, type ReactNode } from "react";
import { useI18n } from "../../i18n/LocaleContext";
import type { MessageKey } from "../../i18n/messages";
import { IconSearch } from "../icons/NavIcons";

/** 可展开搜索框入参 */
type Props = {
  value: string;
  onChange: (value: string) => void;
  /** 占位文案 i18n key */
  placeholderKey: MessageKey;
  className?: string;
  /** 展开输入框按下 Enter 时触发 */
  onSubmit?: () => void;
  /** 外部状态需要持续显示搜索框（例如归档视图） */
  forceOpen?: boolean;
  /** 与搜索输入共享容器的次级动作 */
  trailingAction?: ReactNode;
};

export default function ExpandableSearch({
  value,
  onChange,
  placeholderKey,
  className = "",
  onSubmit,
  forceOpen = false,
  trailingAction,
}: Props) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const inputRef = useRef<HTMLInputElement | null>(null);
  const expanded = forceOpen || open || value.trim().length > 0;

  useEffect(() => {
    if (!expanded) return;
    const id = window.requestAnimationFrame(() => inputRef.current?.focus());
    return () => window.cancelAnimationFrame(id);
  }, [expanded]);

  useEffect(() => {
    if (!expanded) return;
    const onPointerDown = (e: MouseEvent) => {
      if (!rootRef.current?.contains(e.target as Node) && !value.trim()) {
        setOpen(false);
      }
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        if (value) onChange("");
        else setOpen(false);
      }
    };
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [expanded, value, onChange]);

  return (
    <div
      className={`expandable-search ${expanded ? "is-open" : ""} ${className}`.trim()}
      ref={rootRef}
    >
      {expanded ? (
        <div className="expandable-search-field">
          <IconSearch width={15} height={15} className="expandable-search-glyph" />
          <input
            ref={inputRef}
            className="expandable-search-input"
            type="search"
            value={value}
            onChange={(e) => onChange(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                onSubmit?.();
              }
            }}
            placeholder={t(placeholderKey)}
            aria-label={t(placeholderKey)}
          />
          {trailingAction}
          {!forceOpen && (
            <button
              type="button"
              className="expandable-search-close"
              aria-label={t("common.close") as MessageKey}
              onClick={() => {
                onChange("");
                setOpen(false);
              }}
            >
              ×
            </button>
          )}
        </div>
      ) : (
        <button
          type="button"
          className="expandable-search-btn"
          onClick={() => setOpen(true)}
          title={t(placeholderKey)}
          aria-label={t(placeholderKey)}
        >
          <IconSearch width={16} height={16} />
        </button>
      )}
    </div>
  );
}
