/** 消息内导航 / 锚点：macOS Dock 式鱼眼跟随。 */
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
} from "../lib/ui/clampPopover";
import type { ChatMessage } from "../types";

type Props = {
  messages: ChatMessage[];
  listRef: RefObject<HTMLElement | null>;
  bottomRef: RefObject<HTMLElement | null>;
};

const BASE = 18;
/** 与 CSS `.chat-msg-nav-track` gap 一致；放大后仍要留缝 */
const GAP = 12;
/** 与 CSS `.chat-msg-nav-track` padding-top 一致 */
const PAD_TOP = 10;
/** 峰值放大：略收敛，避免挤成一团 */
const MAX_SCALE = 1.52;
/** 影响半径：配合间距做更柔和的鱼眼 */
const RANGE = 88;
/** 同时展示的预览气泡（主 + 邻近） */
const LABEL_MAX = 2;

type TipModel = {
  id: string;
  text: string;
  primary: boolean;
  z: number;
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

/** Idle 槽位中心（放大距离一律相对此基准，避免反馈抖动）。 */
function baseCenterY(index: number): number {
  return PAD_TOP + index * (BASE + GAP) + BASE / 2;
}

/**
 * macOS Dock 余弦衰减：槽位不动，只按与鼠标距离放大。
 * 峰值在鼠标处；走过的地方回落——不是整列跟着鼠标平移。
 */
function dockScale(distance: number): number {
  if (distance >= RANGE) return 1;
  const t = distance / RANGE;
  return 1 + (MAX_SCALE - 1) * 0.5 * (1 + Math.cos(Math.PI * t));
}

export default function ChatMessageNav({
  messages,
  listRef,
  bottomRef,
}: Props) {
  const { t } = useI18n();
  const [activeId, setActiveId] = useState<string | null>(null);
  const [dockActive, setDockActive] = useState(false);
  const [tips, setTips] = useState<TipModel[]>([]);

  const ratiosRef = useRef<Map<string, number>>(new Map());
  const trackRef = useRef<HTMLDivElement>(null);
  const slotRefs = useRef<Map<string, HTMLDivElement>>(new Map());
  const buttonRefs = useRef<Map<string, HTMLButtonElement>>(new Map());
  const tipElsRef = useRef<Map<string, HTMLDivElement>>(new Map());
  const tipMetaRef = useRef<TipModel[]>([]);
  const tipSizeCache = useRef<Map<string, { width: number; height: number }>>(
    new Map(),
  );

  const hoverYRef = useRef<number | null>(null);
  const pendingHoverYRef = useRef<number | null>(null);
  const hoverRafRef = useRef<number | null>(null);
  const focusIdRef = useRef<string | null>(null);
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

    root.querySelectorAll<HTMLElement>("[data-msg-id]").forEach((n) => {
      observer.observe(n);
    });
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

  const paintTip = useCallback(
    (
      id: string,
      visual: {
        top: number;
        left: number;
        side: TipSide;
        opacity: number;
        scale: number;
      },
    ) => {
      const tipEl = tipElsRef.current.get(id);
      if (!tipEl) return;
      tipEl.style.transform = `translate3d(${visual.left}px, ${visual.top}px, 0) scale(${visual.scale})`;
      tipEl.style.opacity = String(visual.opacity);
      tipEl.classList.toggle("is-side-right", visual.side === "right");
    },
    [],
  );

  const tipBounds = useCallback(() => {
    const list = listRef.current;
    if (list) {
      const r = list.getBoundingClientRect();
      if (r.width > 0 && r.height > 0) {
        return {
          left: r.left,
          top: r.top,
          right: r.right,
          bottom: r.bottom,
        };
      }
    }
    const track = trackRef.current;
    if (track) return resolveClipBounds(track);
    return {
      left: 0,
      top: 0,
      right: window.innerWidth,
      bottom: window.innerHeight,
    };
  }, [listRef]);

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

      const bounds = tipBounds();
      const maxDist = Math.max(ranks[ranks.length - 1]?.dist ?? 0, 1);
      const nextTips: TipModel[] = [];

      for (let rank = 0; rank < ranks.length; rank++) {
        const item = ranks[rank]!;
        const btn = buttonRefs.current.get(item.id);
        if (!btn) continue;
        const text = previewsRef.current.get(item.id) ?? "";
        if (!text) continue;

        const t = ranks.length === 1 ? 0 : Math.min(1, item.dist / maxDist);
        const opacity = Math.max(0.35, 1 - t * 0.55);
        const grow = (item.scale - 1) / (MAX_SCALE - 1);
        const labelScale = (0.9 + 0.22 * grow) * (1 - t * 0.08);
        const cached = tipSizeCache.current.get(item.id);
        const tipSize = cached ?? {
          width: Math.min(
            16 * 16,
            window.innerWidth * 0.46,
            Math.max(48, text.length * 7.5 + 22),
          ),
          height: rank === 0 ? 30 : 26,
        };

        const placed = clampFloatingTip({
          anchorRect: btn.getBoundingClientRect(),
          tipSize,
          bounds,
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

      requestAnimationFrame(() => {
        const b = tipBounds();
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
          const grow = (item.scale - 1) / (MAX_SCALE - 1);
          const placed = clampFloatingTip({
            anchorRect: btn.getBoundingClientRect(),
            tipSize: size,
            bounds: b,
            prefer: "left",
            gap: 10,
            pad: 8,
          });
          paintTip(item.id, {
            top: placed.top,
            left: placed.left,
            side: placed.side,
            opacity: Math.max(0.35, 1 - tt * 0.55),
            scale: (0.9 + 0.22 * grow) * (1 - tt * 0.08),
          });
        }
      });
    },
    [paintTip, tipBounds],
  );

  const applyDock = useCallback(
    (hoverY: number | null, focusId: string | null) => {
      const list = messagesRef.current;
      const scales = list.map((m, i) => {
        if (hoverY != null) {
          return dockScale(Math.abs(hoverY - baseCenterY(i)));
        }
        if (focusId && m.id === focusId) return 1.35;
        return 1;
      });

      list.forEach((m, i) => {
        const slot = slotRefs.current.get(m.id);
        if (!slot) return;
        const s = scales[i] ?? 1;
        // 只改缩放，不平移——避免整列跟着鼠标跑
        slot.style.setProperty("--dock-scale", String(s));
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
          .filter((r) => r.dist < RANGE)
          .sort((a, b) => a.dist - b.dist || a.index - b.index)
          .slice(0, Math.min(LABEL_MAX, list.length));
      } else if (focusId) {
        const index = list.findIndex((m) => m.id === focusId);
        if (index >= 0) {
          ranks = [
            { id: focusId, index, dist: 0, scale: scales[index] ?? 1 },
          ];
        }
      }

      layoutTips(ranks);
    },
    [layoutTips],
  );
  const applyDockRef = useRef(applyDock);
  applyDockRef.current = applyDock;

  const flushHover = useCallback(() => {
    hoverRafRef.current = null;
    const y = pendingHoverYRef.current;
    hoverYRef.current = y;
    const active = y != null;
    setDockActive((prev) => (prev === active ? prev : active));
    applyDockRef.current(y, y != null ? null : focusIdRef.current);
  }, []);

  const onTrackMove = useCallback(
    (e: MouseEvent<HTMLDivElement>) => {
      const track = trackRef.current;
      if (!track) return;
      const rect = track.getBoundingClientRect();
      // 相对轨道的连续 Y（含 scroll），鱼眼峰值跟这个走
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
    focusIdRef.current = null;
    setDockActive(false);
    applyDockRef.current(null, null);
  }, []);

  // tips 挂载后按当前鼠标位置再刷一次
  useEffect(() => {
    if (tips.length === 0) return;
    applyDockRef.current(
      hoverYRef.current,
      hoverYRef.current != null ? null : focusIdRef.current,
    );
  }, [tips]);

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
                onFocus={() => {
                  focusIdRef.current = m.id;
                  if (hoverYRef.current == null) {
                    applyDockRef.current(null, m.id);
                  }
                }}
                onBlur={() => {
                  if (focusIdRef.current === m.id) focusIdRef.current = null;
                  if (hoverYRef.current == null) {
                    applyDockRef.current(null, null);
                  }
                }}
                onClick={() => scrollToMessage(m.id)}
              >
                {isUser ? (
                  <User size={9} strokeWidth={2.35} aria-hidden />
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
        <ChevronDown size={10} strokeWidth={2.5} aria-hidden />
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
