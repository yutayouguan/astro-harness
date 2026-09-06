/** 输入框浮动命令面板（/ 斜杠 · @ 提及）。 */
import { useEffect, useMemo, useRef } from "react";
import { useI18n } from "../../i18n/LocaleContext";
import type { SlashAction } from "../../lib/chat/composerCommands";
import type { ComposerContextToken } from "../../lib/chat/composerContext";

export type PaletteKind = "slash" | "mention";

export type PaletteItem = {
  id: string;
  title: string;
  description?: string;
  icon?: string;
  insert?: string;
  action?: SlashAction | "insert" | "help" | "clear";
  skillName?: string;
  mentionKind?: "agent" | "skill" | "mcp";
  /** 选择后以结构化标签加入输入框，而不是写入普通文本。 */
  contextToken?: ComposerContextToken;
  /** 分组标签（如"指令"/"技能"/"添加"/"插件"） */
  group?: string;
};

type Props = {
  kind: PaletteKind;
  items: PaletteItem[];
  query: string;
  activeIndex: number;
  selectedId?: string | null;
  footerHint?: string;
  onHover: (index: number) => void;
  onSelect: (item: PaletteItem) => void;
  onClose: () => void;
};

export function ComposerPalette({
  kind,
  items,
  query,
  activeIndex,
  selectedId,
  onHover,
  onSelect,
  onClose: _onClose,
}: Props) {
  void _onClose;
  const { t } = useI18n();
  const listRef = useRef<HTMLDivElement>(null);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return items;
    return items.filter(
      (it) =>
        it.title.toLowerCase().includes(q) ||
        it.id.toLowerCase().includes(q) ||
        (it.description?.toLowerCase().includes(q) ?? false),
    );
  }, [items, query, kind]);

  useEffect(() => {
    const el = listRef.current?.querySelector<HTMLElement>(
      `[data-idx="${activeIndex}"]`,
    );
    el?.scrollIntoView({ block: "nearest" });
  }, [activeIndex]);

  const groups = useMemo(() => {
    const map = new Map<
      string,
      { label: string; items: { item: PaletteItem; globalIdx: number }[] }
    >();
    filtered.forEach((item, idx) => {
      const g = item.group || "";
      if (!map.has(g)) map.set(g, { label: g, items: [] });
      map.get(g)!.items.push({ item, globalIdx: idx });
    });
    return [...map.values()];
  }, [filtered]);

  return (
    <div
      className={`composer-palette composer-palette--${kind}`}
      role="listbox"
      onMouseDown={(e) => e.preventDefault()}
    >
      <div className="composer-palette-list" ref={listRef}>
        {filtered.length === 0 ? (
          <div className="composer-palette-empty">{t("chat.paletteEmpty")}</div>
        ) : (
          groups.map((group) => (
            <div key={group.label} className="composer-palette-group">
              {group.label && (
                <div className="composer-palette-group-label">
                  {group.label}
                </div>
              )}
              {group.items.map(({ item, globalIdx }) => {
                const active = globalIdx === activeIndex;
                const selected = selectedId === item.id;
                return (
                  <button
                    key={item.id}
                    type="button"
                    role="option"
                    data-idx={globalIdx}
                    aria-selected={active}
                    className={`composer-palette-item ${active ? "is-active" : ""} ${selected ? "is-selected" : ""}`}
                    onMouseEnter={() => onHover(globalIdx)}
                    onClick={() => onSelect(item)}
                  >
                    {item.icon && (
                      <span className="composer-palette-ico" aria-hidden>
                        {item.icon}
                      </span>
                    )}
                    <span className="composer-palette-item-name">
                      {item.title}
                    </span>
                    {item.description && (
                      <span className="composer-palette-item-hint">
                        {item.description}
                      </span>
                    )}
                    {selected && (
                      <span className="composer-palette-check" aria-hidden>
                        ✓
                      </span>
                    )}
                  </button>
                );
              })}
            </div>
          ))
        )}
      </div>
      <div className="composer-palette-foot">
        <kbd>↑↓</kbd> {t("chat.paletteNav")}
        <kbd>↵</kbd> {t("chat.paletteEnter")}
        <kbd>ESC</kbd> {t("chat.paletteEsc")}
      </div>
    </div>
  );
}
