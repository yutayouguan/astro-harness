/** 消息内导航 / 锚点。 */
import {
  useCallback,
  useEffect,
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
const PAD_TOP = 10;
/** Max scale at cursor (macOS-like). */
const MAX_SCALE = 1.72;
/** Influence radius in px along the dock axis. */
const RANGE = 58;
/** Soft falloff (higher = sharper peak). */
const FALLOFF = 1.35;
/** 悬停时最多同时展示的邻近锚点预览数。 */
const LABEL_MAX = 8;

type TipModel = {
  id: string;
  text: string;
  primary: boolean;
  z: number;
};

type TipVisual = {
  top: number;
  left: number;
  side: TipSide;
  opacity: number;
  scale: number;
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

/** Idle-slot center Y for index i（放大用固定基准，避免反馈抖动）。 */
function baseCenterY(index: number): number {
  return PAD_TOP + index * (BASE + GAP) + BASE / 2;
}

/** 按放大后的视觉高度重新堆叠，产生 Dock 起落位移。 */
function dockOffsets(scales: number[]): number[] {
  let y = PAD_TOP;
  return scales.map((s, i) => {
    const h = BASE * s;
    const center = y + h / 2;
    y += h + GAP;
    return center - baseCenterY(i);
  });
}

export default function ChatMessageNav({
  messages,
  listRef,
  bottomRef,
}: Props) {
  const { t } = useI18n();
  const [activeId, setActiveId] = useState<string | null>(null);
  const [dockActive, setDockActive] = useState(false);
  const [hoveredId, setHoveredId] = useState<string | null>(null);
  const [tips, setTips] = useState<TipModel[]>([]);
  const ratiosRef = useRef<Map<string, number>>(new Map());
  const trackRef = useRef<HTMLDivElement>(null);
  const slotRefs = useRef<Map<string, HTMLDivElement>>(new Map());
  const buttonRefs = useRef<Map<string, HTMLButtonElement>>(new Map());
  const tipElsRef = useRef<Map<string, HTMLDivElement>>(new Map());
  const tipMetaRef = useRef<TipModel[]>([]);
  const hoverYRef = useRef<number | null>(null);
  const hoverRafRef = useRef<number | null>(null);
  const pendingHoverYRef = useRef<number | null>(null);
  const tipSizeCache = useRef<Map<string, { width: number; height: number }>>(
    new Map(),
  );
  const messagesRef = useRef(messages);
  messagesRef.current = messages;

  const previews = useMemo(() => {
    const map = new Map<string, string>();
    for (const m of messages) {
      const fallback =
        m.role === "user" ? t("chat.navUser") : t("chat.navAssistant");
      map.set(m.id, previewText(m.content || m.reasoning || "", fallback));
    }
    return map;
  }, [messages, t]);
  const previewsRef = useRef(previews);
  previewsRef.current = previews;

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

  const paintTip = useCallback((id: string, visual: TipVisual) => {
    const tipEl = tipElsRef.current.get(id);
    if (!tipEl) return;
    tipEl.style.transform = `translate3d(${visual.left}px, ${visual.top}px, 0) scale(${visual.scale})`;
    tipEl.style.opacity = String(visual.opacity);
    tipEl.classList.toggle("is-side-right", visual.side === "right");
  }, []);

  const layoutTips = useCallback(
    (
      ranks: { id: string; index: number; dist: number; scale: number }[],
    ) => {
      if (ranks.length === 0) {
        tipSizeCache.current.clear();
        if (tipMetaRef.current.length > 0) {
          tipMetaRef.current = [];
          setTips([]);
        }
        return;
      }

      const maxDist = Math.max(ranks[ranks.length - 1]?.dist ?? 0, 1);
      const nextTips: TipModel[] = [];

      for (let rank = 0; rank < ranks.length; rank++) {
        const item = ranks[rank]!;
        const btn = buttonRefs.current.get(item.id);
        if (!btn) continue;
        const text = previewsRef.current.get(item.id) ?? "";
        if (!text) continue;

        const t = ranks.length === 1 ? 0 : Math.min(1, item.dist / maxDist);
        const opacity = 1 - t * 0.55;
        // 文字随图标鱼眼一起放大，远处略缩小
        const grow = (item.scale - 1) / (MAX_SCALE - 1);
        const labelScale = (0.92 + 0.2 * grow) * (1 - t * 0.1);
        const cached = tipSizeCache.current.get(item.id);
        const tipSize = cached ?? {
          width: Math.min(
            16 * 16,
            window.innerWidth * 0.46,
            Math.max(48, text.length * 7.5 + 22),
          ),
          height: rank === 0 ? 30 : 26,
        };

        // 跟图标视觉框（含 scale/translate）对齐，丝滑起落
        const rect = btn.getBoundingClientRect();
        const placed = clampFloatingTip({
          anchorRect: rect,
          tipSize,
          bounds: resolveClipBounds(btn),
          prefer: "left",
          gap: 10,
          pad: 8,
        });

        nextTips.push({
          id: item.id,
          text,
          primary: rank === 0,
          z: ranks.length - rank,
        });
        paintTip(item.id, {
          top: placed.top,
          left: placed.left,
          side: placed.side,
          opacity,
          scale: labelScale,
        });
      }

      const prev = tipMetaRef.current;
      const same =
        prev.length === nextTips.length &&
        prev.every(
          (p, i) =>
            p.id === nextTips[i]!.id &&
            p.primary === nextTips[i]!.primary &&
            p.text === nextTips[i]!.text,
        );
      if (!same) {
        tipMetaRef.current = nextTips;
        setTips(nextTips);
      }

      // 下一帧用真实尺寸精调一次（只写 DOM，不 setState）
      requestAnimationFrame(() => {
        for (let rank = 0; rank < ranks.length; rank++) {
          const item = ranks[rank]!;
          const tipEl = tipElsRef.current.get(item.id);
          const btn = buttonRefs.current.get(item.id);
          if (!tipEl || !btn) continue;
          const size = measurePopoverSize(tipEl);
          if (size.width < 2 || size.height < 2) continue;
          tipSizeCache.current.set(item.id, size);
          const maxD = Math.max(ranks[ranks.length - 1]?.dist ?? 0, 1);
          const tt =
            ranks.length === 1 ? 0 : Math.min(1, item.dist / maxD);
          const placed = clampFloatingTip({
            anchorRect: btn.getBoundingClientRect(),
            tipSize: size,
            bounds: resolveClipBounds(btn),
            prefer: "left",
            gap: 10,
            pad: 8,
          });
          paintTip(item.id, {
            top: placed.top,
            left: placed.left,
            side: placed.side,
            opacity: 1 - tt * 0.55,
            scale: 1 - tt * 0.12,
          });
        }
      });
    },
    [paintTip],
  );

  /** 每帧直接写 CSS 变量 + 气泡位置，避免 React 重渲染抖动。 */
  const applyDock = useCallback(
    (hoverY: number | null, focusId: string | null) => {
      const list = messagesRef.current;
      const scales: number[] = list.map((m, i) => {
        if (hoverY != null) {
          return dockScale(Math.abs(hoverY - baseCenterY(i)));
        }
        if (focusId && m.id === focusId) return 1.28;
        return 1;
      });
      const tys = hoverY != null ? dockOffsets(scales) : scales.map(() => 0);

      list.forEach((m, i) => {
        const slot = slotRefs.current.get(m.id);
        if (!slot) return;
        const s = scales[i] ?? 1;
        slot.style.setProperty("--dock-scale", String(s));
        slot.style.setProperty("--dock-ty", `${tys[i] ?? 0}px`);
        slot.style.setProperty("--dock-z", String(Math.round(s * 100)));
      });

      let ranks: { id: string; index: number; dist: number; scale: number }[] =
        [];
      if (hoverY != null) {
        ranks = list
          .map((m, i) => ({
            id: m.id,
            index: i,
            dist: Math.abs(hoverY - baseCenterY(i)),
            scale: scales[i] ?? 1,
          }))
          .sort((a, b) => a.dist - b.dist || a.index - b.index)
          .slice(0, Math.min(LABEL_MAX, list.length));
      } else if (focusId) {
        const index = list.findIndex((m) => m.id === focusId);
        if (index >= 0) {
          ranks = [
            {
              id: focusId,
              index,
              dist: 0,
              scale: scales[index] ?? 1,
            },
          ];
        }
      }

      layoutTips(ranks);
    },
    [layoutTips],
  );

  const flushHover = useCallback(() => {
    hoverRafRef.current = null;
    const y = pendingHoverYRef.current;
    hoverYRef.current = y;
    const active = y != null;
    setDockActive((prev) => (prev === active ? prev : active));
    applyDock(y, y != null ? null : hoveredId);
  }, [applyDock, hoveredId]);

  const onTrackMove = useCallback(
    (e: MouseEvent<HTMLDivElement>) => {
      const track = trackRef.current;
      if (!track) return;
      const rect = track.getBoundingClientRect();
      pendingHoverYRef.current = e.clientY - rect.top + track.scrollTop;
      if (hoverRafRef.current != null) return;
      hoverRafRef.current = requestAnimationFrame(flushHover);
    },
    [flushHover],
  );

  const onTrackLeave = useCallback(() => {
    if (hoverRafRef.current != null) {
      cancelAnimationFrame(hoverRafRef.current);
      hoverRafRef.current = null;
    }
    pendingHoverYRef.current = null;
    hoverYRef.current = null;
    setDockActive(false);
    setHoveredId(null);
    applyDock(null, null);
  }, [applyDock]);

  // 键盘聚焦：无鼠标坐标时单独放大当前项
  useEffect(() => {
    if (hoverYRef.current != null) return;
    applyDock(null, hoveredId);
  }, [hoveredId, applyDock]);

  // tips 挂载后立刻按当前 Dock 状态刷一次位置
  useEffect(() => {
    if (tips.length === 0) return;
    applyDock(hoverYRef.current, hoverYRef.current != null ? null : hoveredId);
  }, [tips, applyDock, hoveredId]);

  if (messages.length === 0) return null;

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
          const label = previews.get(m.id) ?? "";

          return (
            <div
              key={m.id}
              ref={(el) => {
                if (el) slotRefs.current.set(m.id, el);
                else slotRefs.current.delete(m.id);
              }}
              className="chat-msg-nav-item"
            >
              <button
                type="button"
                ref={(el) => {
                  if (el) buttonRefs.current.set(m.id, el);
                  else buttonRefs.current.delete(m.id);
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
                  <User size={11} strokeWidth={2.25} aria-hidden />
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

      {tips.length > 0
        ? createPortal(
            <>
              {tips.map((p) => (
                <div
                  key={p.id}
                  ref={(el) => {
                    if (el) tipElsRef.current.set(p.id, el);
                    else tipElsRef.current.delete(p.id);
                  }}
                  className={`chat-msg-nav-label ${p.primary ? "is-primary" : "is-near"}`}
                  style={
                    {
                      zIndex: `calc(var(--z-tip) + ${p.z})`,
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
