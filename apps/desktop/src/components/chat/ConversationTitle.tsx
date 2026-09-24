import { useLayoutEffect, useRef, useState } from "react";

type ConversationTitleProps = {
  title: string;
  renameLabel: string;
  /** 读屏与悬浮提示用的完整名称；展示标题已用图标替代文字时传它 */
  accessibleTitle?: string;
  onRename: () => void;
};

export default function ConversationTitle({
  title,
  renameLabel,
  accessibleTitle,
  onRename,
}: ConversationTitleProps) {
  const titleRef = useRef<HTMLButtonElement | null>(null);
  const [overflowing, setOverflowing] = useState(false);
  const fullTitle = accessibleTitle ?? title;

  useLayoutEffect(() => {
    const node = titleRef.current;
    if (!node) return;

    const measure = () => {
      setOverflowing(node.scrollWidth > node.clientWidth + 1);
    };
    measure();

    if (typeof ResizeObserver === "undefined") {
      window.addEventListener("resize", measure);
      return () => window.removeEventListener("resize", measure);
    }

    const observer = new ResizeObserver(measure);
    observer.observe(node);
    return () => observer.disconnect();
  }, [title]);

  return (
    <button
      ref={titleRef}
      type="button"
      className="content-title-main conversation-title-trigger"
      title={overflowing ? fullTitle : undefined}
      data-tip={overflowing ? fullTitle : undefined}
      data-tip-delay={overflowing ? "400" : undefined}
      aria-label={`${fullTitle} · ${renameLabel}`}
      onClick={onRename}
    >
      {title}
    </button>
  );
}
