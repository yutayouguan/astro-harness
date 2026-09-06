import { Bot } from "lucide-react";
import { useLayoutEffect, useRef } from "react";
import {
  resolveLucideIconById,
  applyPaintToSvg,
} from "../../lib/agent/lucideAgentIcons";
import type { LoopIconData } from "./loopTypes";

interface Props {
  icon: LoopIconData | null;
  size?: number;
}

export default function LoopIcon({ icon, size = 18 }: Props) {
  const wrapRef = useRef<HTMLSpanElement>(null);
  const Icon = icon ? resolveLucideIconById(icon.id) : null;

  useLayoutEffect(() => {
    if (!icon?.paint || !wrapRef.current) return;
    const svg = wrapRef.current.querySelector("svg");
    if (svg) applyPaintToSvg(svg, icon.paint, icon.style ?? "stroke");
  }, [icon]);

  if (!Icon) return <Bot size={size} />;

  return (
    <span ref={wrapRef} style={{ display: "inline-flex" }}>
      <Icon
        size={size}
        strokeWidth={2}
        color={icon?.paint?.kind === "solid" ? icon.paint.color : undefined}
      />
    </span>
  );
}
