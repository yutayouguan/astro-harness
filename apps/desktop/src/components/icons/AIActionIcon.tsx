import { useId, type SVGProps } from "react";

export type AIActionIconProps = Omit<SVGProps<SVGSVGElement>, "children"> & {
  size?: number;
  variant?: "generate" | "search";
  framed?: boolean;
};

/** Decorative AI action glyph; the owning button supplies its accessible name. */
export function AIActionIcon({
  size = 18,
  variant = "generate",
  framed = false,
  className = "",
  ...props
}: AIActionIconProps) {
  const gradientId = `ai-action-${useId().replace(/:/g, "")}`;
  const paint = `url(#${gradientId})`;
  const compact = size <= 16 && !framed;
  return (
    <svg
      {...props}
      width={size}
      height={size}
      viewBox="0 0 32 32"
      fill="none"
      className={`ai-action-icon ${className}`.trim()}
      aria-hidden="true"
      focusable="false"
    >
      <defs>
        <linearGradient
          id={gradientId}
          x1="3"
          y1="16"
          x2="29"
          y2="16"
          gradientUnits="userSpaceOnUse"
        >
          <stop className="ai-icon-stop-cyan" />
          <stop className="ai-icon-stop-blue" offset="0.36" />
          <stop className="ai-icon-stop-purple" offset="0.68" />
          <stop className="ai-icon-stop-pink" offset="1" />
        </linearGradient>
      </defs>
      {framed ? (
        <circle className="ai-icon-plate" cx="16" cy="16" r="15" />
      ) : null}
      <g
        transform={framed ? "translate(6 6) scale(0.625)" : undefined}
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        {variant === "search" ? (
          <>
            <path
              className="ai-icon-stroke"
              stroke={paint}
              strokeWidth="2.2"
              d="M5.8 14.5a10 10 0 1 1 10.6 10M23 23l6 6"
            />
            <path
              className="ai-icon-fill"
              fill={paint}
              d="M8 15.5c.7 4.3 2.2 5.8 6.5 6.5-4.3.7-5.8 2.2-6.5 6.5-.7-4.3-2.2-5.8-6.5-6.5 4.3-.7 5.8-2.2 6.5-6.5Z"
            />
          </>
        ) : (
          <>
            <path
              className="ai-icon-stroke"
              stroke={paint}
              strokeWidth="2.5"
              d="M16 7c0 5-4 9-9 9 5 0 9 4 9 9 0-5 4-9 9-9-5 0-9-4-9-9Z"
            />
            {!compact && (
              <path
                className="ai-icon-stroke"
                stroke={paint}
                strokeWidth="2.2"
                d="M7 7c2 0 4-1 5-3M25 7c0 2 1 4 3 5M25 25c-2 0-4 1-5 3M7 25c0-2-1-4-3-5"
              />
            )}
            {[
              [16, 3],
              [25, 7],
              [29, 16],
              [25, 25],
              [16, 29],
              [7, 25],
              [3, 16],
              [7, 7],
            ]
              .filter((_, index) => !compact || index % 2 === 1)
              .map(([cx, cy]) => (
                <circle
                  key={`${cx}-${cy}`}
                  className="ai-icon-fill"
                  fill={paint}
                  cx={cx}
                  cy={cy}
                  r="1.8"
                />
              ))}
          </>
        )}
      </g>
    </svg>
  );
}
