/** Agent 头像展示：有图用图；默认 Agent 无图时用 Sparkles；否则首字。 */
import { Sparkles } from "lucide-react";
import {
  agentNameInitial,
  resolveAgentIconSrc,
  type AgentIconInfo,
} from "../lib/agent/agentIcons";

/** Agent 头像入参 */
type Props = {
  agent: AgentIconInfo;
  /** 像素边长，默认 22 */
  size?: number;
  className?: string;
};

export default function AgentAvatar({ agent, size = 22, className = "" }: Props) {
  const src = resolveAgentIconSrc(agent);
  if (src) {
    return (
      <img
        className={`agent-avatar ${className}`.trim()}
        src={src}
        alt=""
        width={size}
        height={size}
        draggable={false}
        referrerPolicy="no-referrer"
      />
    );
  }

  const isDefault =
    agent.is_default === true ||
    agent.id === "workspace" ||
    agent.id === "default";

  if (isDefault) {
    const iconSize = Math.max(11, Math.round(size * 0.52));
    return (
      <span
        className={`agent-avatar agent-avatar--default ${className}`.trim()}
        style={{ width: size, height: size }}
        aria-hidden
      >
        <Sparkles size={iconSize} strokeWidth={2.2} />
      </span>
    );
  }

  return (
    <span
      className={`agent-avatar agent-avatar--glyph ${className}`.trim()}
      style={{
        width: size,
        height: size,
        fontSize: Math.max(10, Math.round(size * 0.55)),
      }}
      aria-hidden
    >
      {agentNameInitial(agent.name)}
    </span>
  );
}
