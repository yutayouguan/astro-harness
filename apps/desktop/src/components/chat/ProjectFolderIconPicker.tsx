import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Check, Search, X } from "lucide-react";
import {
  filterMaterialProjectIcons,
  materialProjectIconUrl,
} from "../../lib/projects/materialProjectIcons";

type Props = {
  open: boolean;
  selectedId: string | null;
  onSelect: (iconId: string) => void;
  onClose: () => void;
};

export default function ProjectFolderIconPicker({
  open,
  selectedId,
  onSelect,
  onClose,
}: Props) {
  const [query, setQuery] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  const icons = useMemo(() => filterMaterialProjectIcons(query), [query]);

  useEffect(() => {
    if (!open) return;
    setQuery("");
    const timer = window.setTimeout(() => inputRef.current?.focus(), 40);
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      window.clearTimeout(timer);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [open, onClose]);

  if (!open) return null;

  return createPortal(
    <div
      className="project-icon-picker-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <section
        className="project-icon-picker"
        role="dialog"
        aria-modal="true"
        aria-labelledby="project-icon-picker-title"
      >
        <header className="project-icon-picker-header">
          <div>
            <h3 id="project-icon-picker-title">选择项目图标</h3>
            <p>来自 Material Icon Theme，展开项目时会自动切换为 open 图标。</p>
          </div>
          <button type="button" onClick={onClose} aria-label="关闭图标选择">
            <X size={16} />
          </button>
        </header>

        <label className="project-icon-picker-search">
          <Search size={15} aria-hidden />
          <input
            ref={inputRef}
            type="search"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="搜索图标，例如 home、code、rust"
          />
        </label>

        <div className="project-icon-picker-grid">
          {icons.map((icon) => {
            const selected = selectedId === icon.id;
            return (
              <button
                key={icon.id}
                type="button"
                className={selected ? "is-selected" : ""}
                title={icon.label}
                aria-label={icon.label}
                aria-pressed={selected}
                onClick={() => onSelect(icon.id)}
              >
                <img
                  src={materialProjectIconUrl(icon.id, false)}
                  width={26}
                  height={26}
                  alt=""
                  loading="lazy"
                  draggable={false}
                  aria-hidden
                />
                {selected && (
                  <span className="project-icon-picker-check">
                    <Check size={10} strokeWidth={3} />
                  </span>
                )}
              </button>
            );
          })}
          {icons.length === 0 && (
            <div className="project-icon-picker-empty">没有匹配的图标</div>
          )}
        </div>
      </section>
    </div>,
    document.body,
  );
}
