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
  const validIcon = icon && MCP_ICON_ID.test(icon) ? icon : undefined;
  const remoteIcon = icon && HTTPS_ICON_URL.test(icon) ? icon : undefined;
  const resolvedIcon = validIcon ?? remoteIcon;

  if (!resolvedIcon || failedIcon === resolvedIcon) {
    return <McpIcon size={size} className={className} style={style} />;
  }

  return (
    <img
      alt=""
      aria-hidden
      className={["mcp-brand-icon", className].filter(Boolean).join(" ")}
      height={size}
      width={size}
      src={validIcon ? `/mcp-icons/${validIcon}.svg` : remoteIcon}
      style={style}
      onError={() => setFailedIcon(resolvedIcon)}
    />
  );
}
