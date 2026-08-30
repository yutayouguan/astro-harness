import { useState, type CSSProperties } from "react";
import McpIcon from "./McpIcon";

const MCP_ICON_ID = /^[a-z0-9-]+$/;
const HTTPS_ICON_URL = /^https:\/\//i;

type Props = {
  icon?: string;
  size?: number;
  className?: string;
  style?: CSSProperties;
};

/** Resolve a bundled icon id or HTTPS catalog icon with a safe MCP fallback. */
export default function McpBrandIcon({
  icon,
  size = 22,
  className,
  style,
}: Props) {
  const [failedIcon, setFailedIcon] = useState<string>();
  const [loadedIcon, setLoadedIcon] = useState<string>();
  const validIcon = icon && MCP_ICON_ID.test(icon) ? icon : undefined;
  const remoteIcon = icon && HTTPS_ICON_URL.test(icon) ? icon : undefined;
  const resolvedIcon = validIcon ?? remoteIcon;

  if (!resolvedIcon || failedIcon === resolvedIcon) {
    return <McpIcon size={size} className={className} style={style} />;
  }

  return (
    <span
      aria-hidden
      className={["mcp-brand-icon-shell", className].filter(Boolean).join(" ")}
      style={style}
    >
      {loadedIcon !== resolvedIcon ? (
        <McpIcon className="mcp-brand-icon-placeholder" size={Math.max(12, size - 6)} />
      ) : null}
      <img
        alt=""
        className={`mcp-brand-icon${loadedIcon === resolvedIcon ? " is-loaded" : ""}`}
        decoding="async"
        height={size}
        loading={remoteIcon ? "lazy" : "eager"}
        referrerPolicy="no-referrer"
        src={validIcon ? `/mcp-icons/${validIcon}.svg` : remoteIcon}
        width={size}
        onError={() => setFailedIcon(resolvedIcon)}
        onLoad={() => setLoadedIcon(resolvedIcon)}
      />
    </span>
  );
}
