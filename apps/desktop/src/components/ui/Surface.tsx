import { forwardRef, type HTMLAttributes } from "react";

export type SurfaceVariant = "panel" | "card" | "elevated";

export type SurfaceProps = HTMLAttributes<HTMLDivElement> & {
  variant?: SurfaceVariant;
  interactive?: boolean;
};

export const Surface = forwardRef<HTMLDivElement, SurfaceProps>(
  function Surface(
    {
      variant = "panel",
      interactive = false,
      className = "",
      ...props
    },
    ref,
  ) {
    return (
      <div
        {...props}
        ref={ref}
        className={[
          "ui-surface",
          `ui-surface--${variant}`,
          interactive ? "is-interactive" : "",
          className,
        ]
          .filter(Boolean)
          .join(" ")}
      />
    );
  },
);
