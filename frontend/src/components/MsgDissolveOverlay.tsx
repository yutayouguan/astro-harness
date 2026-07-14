/** 编辑截断时：气泡内文字与微粒向外消散。 */
import { useMemo, type CSSProperties } from "react";

export const MSG_DISSOLVE_MS = 900;

type Particle = {
  id: number;
  kind: "char" | "dust";
  ch?: string;
  x: number;
  y: number;
  dx: number;
  dy: number;
  rot: number;
  delay: number;
  size: number;
};

/** 从正文抽可显示字符（限量），用作飞行文字粒子 */
function sampleChars(text: string, limit: number): string[] {
  const chars: string[] = [];
  for (const ch of text) {
    if (/\s/.test(ch)) continue;
    chars.push(ch);
    if (chars.length >= limit) break;
  }
  return chars;
}

/** 基于 messageId 的简单伪随机，保证同一次消散粒子稳定 */
function mulberry32(seed: number) {
  return () => {
    let t = (seed += 0x6d2b79f5);
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function seedFromId(id: string): number {
  let h = 2166136261;
  for (let i = 0; i < id.length; i += 1) {
    h ^= id.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

type Props = {
  messageId: string;
  text: string;
  /** 同批消散中的顺序，用于错峰 */
  staggerIndex?: number;
};

/** 覆盖在气泡上的文字 / 微尘粒子层 */
export default function MsgDissolveOverlay({
  messageId,
  text,
  staggerIndex = 0,
}: Props) {
  const particles = useMemo(() => {
    const rnd = mulberry32(seedFromId(messageId) || 1);
    const chars = sampleChars(text, 28);
    const out: Particle[] = [];
    let id = 0;
    for (const ch of chars) {
      const angle = rnd() * Math.PI * 2;
      const dist = 36 + rnd() * 110;
      out.push({
        id: id++,
        kind: "char",
        ch,
        x: 8 + rnd() * 84,
        y: 12 + rnd() * 76,
        dx: Math.cos(angle) * dist,
        dy: Math.sin(angle) * dist - 28 - rnd() * 40,
        rot: (rnd() - 0.5) * 420,
        delay: staggerIndex * 0.05 + rnd() * 0.18,
        size: 11 + rnd() * 5,
      });
    }
    const dustCount = 36;
    for (let i = 0; i < dustCount; i += 1) {
      const angle = rnd() * Math.PI * 2;
      const dist = 24 + rnd() * 130;
      out.push({
        id: id++,
        kind: "dust",
        x: rnd() * 100,
        y: rnd() * 100,
        dx: Math.cos(angle) * dist,
        dy: Math.sin(angle) * dist - 20 - rnd() * 50,
        rot: rnd() * 180,
        delay: staggerIndex * 0.05 + rnd() * 0.22,
        size: 2 + rnd() * 3.5,
      });
    }
    return out;
  }, [messageId, text, staggerIndex]);

  return (
    <div className="msg-dissolve-layer" aria-hidden>
      {particles.map((p) => (
        <span
          key={p.id}
          className={`msg-dissolve-particle is-${p.kind}`}
          style={
            {
              left: `${p.x}%`,
              top: `${p.y}%`,
              fontSize: p.kind === "char" ? `${p.size}px` : undefined,
              width: p.kind === "dust" ? p.size : undefined,
              height: p.kind === "dust" ? p.size : undefined,
              animationDelay: `${p.delay}s`,
              "--dx": `${p.dx}px`,
              "--dy": `${p.dy}px`,
              "--rot": `${p.rot}deg`,
            } as CSSProperties
          }
        >
          {p.kind === "char" ? p.ch : null}
        </span>
      ))}
    </div>
  );
}
