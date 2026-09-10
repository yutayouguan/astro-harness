import { useEffect, useRef, useState, type ReactNode } from "react";

/** Small disclosure of ordinary buttons, not an ARIA menu with custom navigation. */
export default function PetMoreMenu({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const [above, setAbove] = useState(false);
  const root = useRef<HTMLDivElement>(null),
    trigger = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (!open) return;
    const bounds = root.current?.getBoundingClientRect();
    const menuHeight =
      root.current?.querySelector<HTMLElement>(".pet-more-content")
        ?.offsetHeight ?? 220;
    setAbove(
      !!bounds &&
        bounds.bottom + menuHeight + 12 > window.innerHeight &&
        bounds.top > menuHeight,
    );
    const dismiss = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("pointerdown", dismiss);
    return () => document.removeEventListener("pointerdown", dismiss);
  }, [open]);
  return (
    <div
      className="pet-more"
      data-above={above}
      ref={root}
      onKeyDown={(e) => {
        if (e.key === "Escape" && open) {
          e.stopPropagation();
          setOpen(false);
          trigger.current?.focus();
        }
      }}
      onBlur={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget)) setOpen(false);
      }}
    >
      <button
        type="button"
        ref={trigger}
        aria-label={label}
        aria-expanded={open}
        onClick={() => setOpen(!open)}
      >
        •••
      </button>
      {open && (
        <div
          className="pet-more-content"
          onClick={(e) => {
            if ((e.target as Element).closest("button:not(:disabled)")) {
              setOpen(false);
              trigger.current?.focus();
            }
          }}
        >
          {children}
        </div>
      )}
    </div>
  );
}
