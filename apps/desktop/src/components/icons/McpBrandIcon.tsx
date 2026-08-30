import { useState, type CSSProperties } from "react";
import McpIcon from "./McpIcon";

const MCP_ICON_ID = /^[a-z0-9-]+$/;

type Props = {
  icon?: string;
  size?: number;
  className?: string;
  style?: CSSProperties;
};

/** Resolve a catalog-provided icon id to a bundled asset with a safe MCP fallback. */
export default function McpBrandIcon({
  icon,
  size = 22,
  className,
  style,
}: Props) {
  const [failedIcon, setFailedIcon] = useState<string>();
  const validIcon = icon && MCP_ICON_ID.test(icon) ? icon : undefined;

  if (!validIcon || failedIcon === validIcon) {
    return <McpIcon size={size} className={className} style={style} />;
  }

  return (
    <img
      alt=""
      aria-hidden
      className={["mcp-brand-icon", className].filter(Boolean).join(" ")}
      height={size}
      width={size}
      src={`/mcp-icons/${validIcon}.svg`}
      style={style}
      onError={() => setFailedIcon(validIcon)}
    />
  );
}
