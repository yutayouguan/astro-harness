import { useId, useRef, type KeyboardEvent, type ReactNode } from "react";
import {
  isSegmentedTabNavigationKey,
  nextEnabledTabIndex,
} from "../../lib/ui/segmentedTabs";

export type SegmentedTabItem = {
  value: string;
  label: ReactNode;
  icon?: ReactNode;
  count?: ReactNode;
  disabled?: boolean;
  panelId?: string;
  className?: string;
};

export type SegmentedTabsProps = {
  items: readonly SegmentedTabItem[];
  value: string;
  onValueChange: (value: string) => void;
  size?: "sm" | "md";
  className?: string;
  "aria-label": string;
};

export function SegmentedTabs({
  items,
  value,
  onValueChange,
  size = "md",
  className = "",
  "aria-label": ariaLabel,
}: SegmentedTabsProps) {
  const id = useId();
  const refs = useRef<Array<HTMLButtonElement | null>>([]);
  const selectedIndex = items.findIndex((item) => item.value === value);
  const fallbackIndex = items.findIndex((item) => !item.disabled);
  const selectedIsFocusable =
    selectedIndex >= 0 && !items[selectedIndex]?.disabled;

  const moveFocus = (
    event: KeyboardEvent<HTMLButtonElement>,
    currentIndex: number,
  ) => {
    if (!isSegmentedTabNavigationKey(event.key)) return;
    event.preventDefault();
    const nextIndex = nextEnabledTabIndex(
      currentIndex,
      event.key,
      items.map((item) => Boolean(item.disabled)),
    );
    if (nextIndex < 0) return;
    const item = items[nextIndex];
    refs.current[nextIndex]?.focus();
    if (item) onValueChange(item.value);
  };

  return (
    <div
      className={["ui-segmented-tabs", `ui-segmented-tabs--${size}`, className]
        .filter(Boolean)
        .join(" ")}
      role="tablist"
      aria-label={ariaLabel}
      aria-orientation="horizontal"
    >
      {items.map((item, index) => {
        const selected = item.value === value;
        return (
          <button
            key={item.value}
            ref={(node) => {
              refs.current[index] = node;
            }}
            id={`${id}-tab-${index}`}
            className={[
              "ui-segmented-tabs__tab",
              selected ? "is-active active" : "",
              item.className ?? "",
            ]
              .filter(Boolean)
              .join(" ")}
            type="button"
            role="tab"
            aria-selected={selected}
            aria-controls={item.panelId}
            tabIndex={
              (selected && !item.disabled) ||
              (!selectedIsFocusable && index === fallbackIndex)
                ? 0
                : -1
            }
            disabled={item.disabled}
            onClick={() => onValueChange(item.value)}
            onKeyDown={(event) => moveFocus(event, index)}
          >
            {item.icon ? (
              <span className="ui-segmented-tabs__icon" aria-hidden="true">
                {item.icon}
              </span>
            ) : null}
            <span className="ui-segmented-tabs__label">{item.label}</span>
            {item.count != null ? (
              <span className="ui-segmented-tabs__count">{item.count}</span>
            ) : null}
          </button>
        );
      })}
    </div>
  );
}
