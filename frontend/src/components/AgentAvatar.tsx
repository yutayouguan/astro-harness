/** Agent 头像展示。 */
import { agentNameInitial, resolveAgentIconSrc, type AgentIconInfo } from "../lib/agentIcons";

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
  return (
    <span
      className={`agent-avatar agent-avatar--glyph ${className}`.trim()}
      style={{ width: size, height: size, fontSize: Math.max(10, Math.round(size * 0.55)) }}
      aria-hidden
    >
      {agentNameInitial(agent.name)}
    </span>
  );
}
