import { useLayoutEffect, useRef, useState } from "react";

type ConversationTitleProps = {
  title: string;
  renameLabel: string;
  onRename: () => void;
};

export default function ConversationTitle({
  title,
  renameLabel,
  onRename,
}: ConversationTitleProps) {
  const titleRef = useRef<HTMLButtonElement | null>(null);
  const [overflowing, setOverflowing] = useState(false);

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
      title={overflowing ? title : undefined}
      data-tip={overflowing ? title : undefined}
      data-tip-delay={overflowing ? "400" : undefined}
      aria-label={`${title} · ${renameLabel}`}
      onClick={onRename}
    >
      {title}
    </button>
  );
}
