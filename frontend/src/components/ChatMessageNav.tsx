/** 消息内导航 / 锚点。 */
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type MouseEvent,
  type RefObject,
} from "react";
import { createPortal } from "react-dom";
import { ChevronDown, User } from "lucide-react";
import { useI18n } from "../i18n/LocaleContext";
import {
  clampFloatingTip,
  measurePopoverSize,
  resolveClipBounds,
  type TipSide,
} from "../lib/clampPopover";
import type { ChatMessage } from "../types";

/** 消息码头导航入参 */
type Props = {
  messages: ChatMessage[];
  /** 消息列表滚动容器 */
  listRef: RefObject<HTMLElement | null>;
  /** 列表底部锚点（用于滚到底） */
  bottomRef: RefObject<HTMLElement | null>;
};

/** Base icon size (px). */
const BASE = 22;
/** Vertical gap between slots when idle. */
const GAP = 6;
/** Track padding-top (must match CSS). */
const PAD_TOP = 4;
/** Max scale at cursor (macOS-like). */
const MAX_SCALE = 1.72;
/** Influence radius in px along the dock axis. */
const RANGE = 58;
/** Soft falloff (higher = sharper peak). */
const FALLOFF = 1.35;
/** 悬停时最多同时展示的邻近锚点预览数。 */
const LABEL_MAX = 10;

type LabelPlacement = {
  id: string;
  text: string;
  top: number;
  left: number;
  side: TipSide;
  opacity: number;
  scale: number;
  z: number;
  primary: boolean;
};

function previewText(content: string, fallback: string): string {
  const plain = content
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/`[^`]*`/g, " ")
    .replace(/!\[[^\]]*\]\([^)]*\)/g, " ")
    .replace(/\[[^\]]*\]\([^)]*\)/g, " ")
    .replace(/[#>*_\-~|]+/g, " ")
    .replace(/\s+/g, " ")
    .trim();
  if (!plain) return fallback;
  return plain.length > 32 ? `${plain.slice(0, 32)}…` : plain;
}

function dockScale(distance: number): number {
  if (distance >= RANGE) return 1;
  const t = 1 - distance / RANGE;
  return 1 + (MAX_SCALE - 1) * t ** FALLOFF;
}

/** Idle-slot center Y for index i, independent of current scales (avoids jitter). */
function baseCenterY(index: number): number {
  return PAD_TOP + index * (BASE + GAP) + BASE / 2;
}

export default function ChatMessageNav({
  messages,
  listRef,
  bottomRef,
}: Props) {
  const { t } = useI18n();
  const [activeId, setActiveId] = useState<string | null>(null);
  const [hoverY, setHoverY] = useState<number | null>(null);
  const [hoveredId, setHoveredId] = useState<string | null>(null);
  const [labelPlacements, setLabelPlacements] = useState<LabelPlacement[]>([]);
  const ratiosRef = useRef<Map<string, number>>(new Map());
  const trackRef = useRef<HTMLDivElement>(null);
  const itemRefs = useRef<Map<string, HTMLButtonElement>>(new Map());
  const labelRefs = useRef<Map<string, HTMLDivElement>>(new Map());

  const previews = useMemo(() => {
    const map = new Map<string, string>();
    for (const m of messages) {
      const fallback =
        m.role === "user" ? t("chat.navUser") : t("chat.navAssistant");
      map.set(m.id, previewText(m.content || m.reasoning || "", fallback));
    }
    return map;
  }, [messages, t]);

  useEffect(() => {
    const root = listRef.current;
    if (!root || messages.length === 0) return;

    ratiosRef.current.clear();

    const pickActive = () => {
      let bestId: string | null = null;
      let bestRatio = 0;
      for (const [id, ratio] of ratiosRef.current) {
        if (ratio > bestRatio) {
          bestRatio = ratio;
          bestId = id;
        }
      }
      if (bestId) setActiveId(bestId);
    };

    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          const id = (entry.target as HTMLElement).dataset.msgId;
          if (!id) continue;
          ratiosRef.current.set(
            id,
            entry.isIntersecting ? entry.intersectionRatio : 0,
          );
        }
        pickActive();
      },
      {
        root,
        rootMargin: "-28% 0px -28% 0px",
        threshold: [0, 0.15, 0.35, 0.55, 0.75, 1],
      },
    );

    const nodes = root.querySelectorAll<HTMLElement>("[data-msg-id]");
    nodes.forEach((n) => observer.observe(n));

    return () => observer.disconnect();
  }, [listRef, messages]);

  const scrollToMessage = useCallback((id: string) => {
    const el = document.getElementById(`msg-${id}`);
    if (!el) return;
    el.scrollIntoView({ behavior: "smooth", block: "center" });
    setActiveId(id);
  }, []);

  const scrollToBottom = useCallback(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth", block: "end" });
    if (messages.length > 0) {
      setActiveId(messages[messages.length - 1]!.id);
    }
  }, [bottomRef, messages]);

  const onTrackMove = useCallback((e: MouseEvent<HTMLDivElement>) => {
    const track = trackRef.current;
    if (!track) return;
    const rect = track.getBoundingClientRect();
    setHoverY(e.clientY - rect.top + track.scrollTop);
  }, []);

  const onTrackLeave = useCallback(() => {
    setHoverY(null);
    setHoveredId(null);
    setLabelPlacements([]);
  }, []);

  const scales = useMemo(() => {
    const result = new Map<string, number>();
    if (hoverY == null) {
      for (const m of messages) result.set(m.id, 1);
      return result;
    }
    messages.forEach((m, i) => {
      result.set(m.id, dockScale(Math.abs(hoverY - baseCenterY(i))));
    });
    return result;
  }, [hoverY, messages]);

  /** 按与悬停点距离排序的邻近锚点（最多 LABEL_MAX）；键盘聚焦时只显示一项。 */
  const nearbyRanks = useMemo(() => {
    if (hoverY != null) {
      return messages
        .map((m, i) => ({
          id: m.id,
          index: i,
          dist: Math.abs(hoverY - baseCenterY(i)),
        }))
        .sort((a, b) => a.dist - b.dist || a.index - b.index)
        .slice(0, Math.min(LABEL_MAX, messages.length));
    }
    if (hoveredId) {
      const index = messages.findIndex((m) => m.id === hoveredId);
      if (index < 0) return [];
      return [{ id: hoveredId, index, dist: 0 }];
    }
    return [];
  }, [hoverY, hoveredId, messages]);

  // Portal 标签：先估宽定位，量完真实尺寸再精调一次。
  useLayoutEffect(() => {
    if (nearbyRanks.length === 0) {
      setLabelPlacements([]);
      return;
    }

    const maxDist = Math.max(nearbyRanks[nearbyRanks.length - 1]?.dist ?? 0, 1);
    const estimated: LabelPlacement[] = [];

    for (let rank = 0; rank < nearbyRanks.length; rank++) {
      const item = nearbyRanks[rank]!;
      const el = itemRefs.current.get(item.id);
      if (!el) continue;
      const text = previews.get(item.id) ?? "";
      if (!text) continue;

      const rect = el.getBoundingClientRect();
      const t =
        nearbyRanks.length === 1 ? 0 : Math.min(1, item.dist / maxDist);
      const opacity = 1 - t * 0.58;
      const scale = 1 - t * 0.14;
      const approxW = Math.min(
        16 * 16,
        window.innerWidth * 0.46,
        Math.max(48, text.length * 7.5 + 22),
      );
      const approxH = rank === 0 ? 30 : 26;
      const placed = clampFloatingTip({
        anchorRect: rect,
        tipSize: { width: approxW, height: approxH },
        bounds: resolveClipBounds(el),
        prefer: "left",
        gap: 12,
        pad: 8,
      });

      estimated.push({
        id: item.id,
        text,
        top: placed.top,
        left: placed.left,
        side: placed.side,
        opacity,
        scale,
        z: nearbyRanks.length - rank,
        primary: rank === 0,
      });
    }

    setLabelPlacements(estimated);

    const raf = requestAnimationFrame(() => {
      const refined: LabelPlacement[] = [];
      let dirty = false;
      for (const p of estimated) {
        const tipEl = labelRefs.current.get(p.id);
        const anchor = itemRefs.current.get(p.id);
        if (!tipEl || !anchor) {
          refined.push(p);
          continue;
        }
        const size = measurePopoverSize(tipEl);
        if (size.width < 2 || size.height < 2) {
          refined.push(p);
          continue;
        }
        const placed = clampFloatingTip({
          anchorRect: anchor.getBoundingClientRect(),
          tipSize: size,
          bounds: resolveClipBounds(anchor),
          prefer: "left",
          gap: 12,
          pad: 8,
        });
        if (
          Math.abs(placed.left - p.left) > 0.5 ||
          Math.abs(placed.top - p.top) > 0.5 ||
          placed.side !== p.side
        ) {
          dirty = true;
        }
        refined.push({
          ...p,
          left: placed.left,
          top: placed.top,
          side: placed.side,
        });
      }
      if (dirty) setLabelPlacements(refined);
    });
    return () => cancelAnimationFrame(raf);
  }, [nearbyRanks, scales, previews]);

  if (messages.length === 0) return null;

  const dockActive = hoverY != null;

  return (
    <nav
      className={`chat-msg-nav ${dockActive ? "is-dock-active" : ""}`}
      aria-label={t("chat.navLabel")}
    >
      <div
        ref={trackRef}
        className="chat-msg-nav-track"
        onMouseMove={onTrackMove}
        onMouseLeave={onTrackLeave}
      >
        {messages.map((m) => {
          const isActive = activeId === m.id;
          const isUser = m.role === "user";
          const scale = scales.get(m.id) ?? 1;
          const label = previews.get(m.id) ?? "";
          const slot = BASE * scale;
          const style = {
            "--dock-scale": String(scale),
            "--dock-z": String(Math.round(scale * 100)),
            width: slot,
            height: slot,
          } as CSSProperties;

          return (
            <div
              key={m.id}
              className="chat-msg-nav-item"
              style={style}
            >
              <button
                type="button"
                ref={(el) => {
                  if (el) itemRefs.current.set(m.id, el);
                  else itemRefs.current.delete(m.id);
                }}
                className={`chat-msg-nav-dot ${isUser ? "is-user" : "is-assistant"} ${
                  isActive ? "is-active" : ""
                }`}
                aria-label={label}
                aria-current={isActive ? "true" : undefined}
                onMouseEnter={() => setHoveredId(m.id)}
                onMouseLeave={() =>
                  setHoveredId((cur) => (cur === m.id ? null : cur))
                }
                onFocus={() => setHoveredId(m.id)}
                onBlur={() => setHoveredId(null)}
                onClick={() => scrollToMessage(m.id)}
              >
                {isUser ? (
                  <User
                    size={Math.round(10 + 3 * (scale - 1))}
                    strokeWidth={2.25}
                    aria-hidden
                  />
                ) : (
                  <span className="chat-msg-nav-glyph" aria-hidden>
                    iC
                  </span>
                )}
              </button>
            </div>
          );
        })}
      </div>
      <button
        type="button"
        className="chat-msg-nav-bottom"
        title={t("chat.navScrollBottom")}
        aria-label={t("chat.navScrollBottom")}
        onClick={scrollToBottom}
      >
        <ChevronDown size={12} strokeWidth={2.4} aria-hidden />
      </button>

      {labelPlacements.length > 0
        ? createPortal(
            <>
              {labelPlacements.map((p) => (
                <div
                  key={p.id}
                  ref={(el) => {
                    if (el) labelRefs.current.set(p.id, el);
                    else labelRefs.current.delete(p.id);
                  }}
                  className={`chat-msg-nav-label ${p.primary ? "is-primary" : "is-near"}${
                    p.side === "right" ? " is-side-right" : ""
                  }`}
                  style={
                    {
                      top: p.top,
                      left: p.left,
                      zIndex: `calc(var(--z-tip) + ${p.z})`,
                      "--label-opacity": String(p.opacity),
                      "--label-scale": String(p.scale),
                    } as CSSProperties
                  }
                  role="tooltip"
                >
                  <span className="chat-msg-nav-label-text">{p.text}</span>
                </div>
              ))}
            </>,
            document.body,
          )
        : null}
    </nav>
  );
}
