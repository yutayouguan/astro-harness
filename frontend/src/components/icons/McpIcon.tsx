/** MCP 品牌图标（@lobehub/icons）。 */
import type { CSSProperties } from "react";
import Mcp from "@lobehub/icons/es/MCP/components/Mono";

type Props = {
  size?: number | string;
  className?: string;
  style?: CSSProperties;
  "aria-hidden"?: boolean | "true" | "false";
};

export default function McpIcon({
  size = 14,
  className,
  style,
  "aria-hidden": ariaHidden = true,
}: Props) {
  return (
    <span aria-hidden={ariaHidden} className={className} style={style}>
      <Mcp size={size} />
    </span>
  );
}
