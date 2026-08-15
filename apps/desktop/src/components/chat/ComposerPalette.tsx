/** 输入框模式 / 附件调色板。 */
import { useEffect, useMemo, useRef } from "react";
import { useI18n } from "../../i18n/LocaleContext";
import type { ThinkingLevel } from "../../lib/chat/thinkingPrefs";
import type { SlashAction } from "../../lib/chat/composerCommands";

/** 调色板种类：斜杠命令 / @提及 / 思考档位 */
export type PaletteKind = "slash" | "mention" | "thinking";

/** 调色板单项 */
export type PaletteItem = {
  id: string;
  title: string;
  description?: string;
  icon?: string;
  insert?: string;
  /** slash 命令动作 */
  action?: SlashAction | "insert" | "help" | "clear";
  /** insert_skill 时的技能名 */
  skillName?: string;
  /** @ 提及类别 */
  mentionKind?: "agent" | "skill" | "mcp";
  /** thinking 等级 */
  level?: ThinkingLevel;
};

/** Composer 浮动调色板入参 */
type Props = {
  kind: PaletteKind;
  items: PaletteItem[];
  /** 过滤查询串 */
  query: string;
  /** 键盘高亮下标 */
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
  footerHint,
  onHover,
  onSelect,
  onClose,
}: Props) {
  const { t } = useI18n();
  const listRef = useRef<HTMLDivElement>(null);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q || kind === "thinking") return items;
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

  const title =
    kind === "slash"
      ? t("chat.slashTitle")
      : kind === "mention"
        ? t("chat.mentionTitle")
        : t("chat.thinkingLength");

  return (
    <div
      className={`composer-palette composer-palette--${kind}`}
      role="listbox"
      aria-label={title}
    >
      <div className="composer-palette-head">{title}</div>
      <div className="composer-palette-list" ref={listRef}>
        {filtered.length === 0 ? (
          <div className="composer-palette-empty">{t("chat.paletteEmpty")}</div>
        ) : (
          filtered.map((item, index) => {
            const active = index === activeIndex;
            const selected = selectedId === item.id || selectedId === item.level;
            return (
              <button
                key={item.id}
                type="button"
                role="option"
                data-idx={index}
                data-kind={item.mentionKind ?? undefined}
                aria-selected={active}
                className={`composer-palette-item ${active ? "is-active" : ""} ${
                  selected ? "is-selected" : ""
                } ${item.mentionKind ? `is-${item.mentionKind}` : ""}`}
                onMouseEnter={() => onHover(index)}
                onClick={() => onSelect(item)}
              >
                <span className="composer-palette-item-main">
                  <span className="composer-palette-item-title">
                    {item.icon ? <span className="composer-palette-ico">{item.icon}</span> : null}
                    {item.title}
                    {item.mentionKind === "skill" ? (
                      <span className="composer-palette-badge composer-palette-badge--skill">skill</span>
                    ) : item.mentionKind === "mcp" ? (
                      <span className="composer-palette-badge composer-palette-badge--mcp">MCP</span>
                    ) : null}
                  </span>
                  {item.description ? (
                    <span className="composer-palette-item-desc">{item.description}</span>
                  ) : null}
                </span>
                {selected ? (
                  <span className="composer-palette-check" aria-hidden>
                    ✓
                  </span>
                ) : null}
              </button>
            );
          })
        )}
      </div>
      <div className="composer-palette-foot">
        <span>{footerHint ?? title}</span>
        <span className="composer-palette-keys">
          ESC {t("chat.paletteEsc")} · ↑↓ {t("chat.paletteNav")} · ↵ {t("chat.paletteEnter")}
        </span>
        <button type="button" className="composer-palette-close" onClick={onClose}>
          ESC
        </button>
      </div>
    </div>
  );
}
