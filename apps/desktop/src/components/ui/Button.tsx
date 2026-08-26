import {
  forwardRef,
  type ButtonHTMLAttributes,
  type ReactNode,
} from "react";

export type ButtonVariant = "primary" | "secondary" | "ghost" | "danger";
export type ButtonSize = "sm" | "md" | "lg";

export type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: ButtonVariant;
  size?: ButtonSize;
  active?: boolean;
  busy?: boolean;
  busyLabel?: string;
};

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  function Button(
    {
      variant = "secondary",
      size = "md",
      active = false,
      busy = false,
      busyLabel = "处理中",
      className = "",
      children,
      disabled,
      type = "button",
      "aria-pressed": ariaPressed,
      ...props
    },
    ref,
  ) {
    return (
      <button
        {...props}
        ref={ref}
        type={type}
        className={[
          "ui-button",
          `ui-button--${variant}`,
          `ui-button--${size}`,
          active ? "is-active" : "",
          busy ? "is-busy" : "",
          className,
        ]
          .filter(Boolean)
          .join(" ")}
        disabled={disabled || busy}
        aria-busy={busy || undefined}
        aria-pressed={ariaPressed ?? (active ? true : undefined)}
      >
        {busy ? <span className="ui-button__spinner" aria-hidden="true" /> : null}
        <span className="ui-button__content">{children}</span>
        {busy ? <span className="ui-visually-hidden">{busyLabel}</span> : null}
      </button>
    );
  },
);

export type IconButtonProps = Omit<ButtonProps, "children"> & {
  "aria-label": string;
  children: ReactNode;
};

export const IconButton = forwardRef<HTMLButtonElement, IconButtonProps>(
  function IconButton({ className = "", children, ...props }, ref) {
    return (
      <Button
        {...props}
        ref={ref}
        className={`ui-icon-button ${className}`.trim()}
      >
        {children}
      </Button>
    );
  },
);
