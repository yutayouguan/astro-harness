import {
  memo,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import { createPortal } from "react-dom";
import { motion, useReducedMotion } from "framer-motion";
import { AstroLogoMark } from "../icons/AstroLogoMark";
import { WelcomeLogoEffect } from "../chat/WelcomeLogoEffect";
const INTRO_PARTICLES = Array.from({ length: 28 }, (_, index) => {
  const angle = (index / 28) * Math.PI * 2 + (index % 3) * 0.18;
  const radius = 118 + (index % 6) * 25;
  return {
    x: Math.cos(angle) * radius,
    y: Math.sin(angle) * radius * 0.72,
    delay: (index % 7) * 34,
    size: 2 + (index % 3),
  };
});
export function OnboardingLogo({ compact = false }: { compact?: boolean }) {
  return (
    <div
      className={`onboarding-logo ${compact ? "is-compact" : ""}`}
      aria-hidden
    >
      {!compact ? (
        <span className="onboarding-particle-field">
          {INTRO_PARTICLES.map((particle, index) => (
            <i
              key={index}
              style={
                {
                  "--particle-x": `${particle.x.toFixed(1)}px`,
                  "--particle-y": `${particle.y.toFixed(1)}px`,
                  "--particle-delay": `${particle.delay}ms`,
                  "--particle-size": `${particle.size}px`,
                } as CSSProperties
              }
            />
          ))}
        </span>
      ) : null}
      <span className="onboarding-logo-halo" />
      <AstroLogoMark className="onboarding-logo-base" />
      {!compact && (
        <span className="chat-welcome-mark onboarding-logo-mark">
          <span className="chat-welcome-mark-glow" />
          <span className="chat-welcome-illust">
            <WelcomeLogoEffect />
          </span>
        </span>
      )}
    </div>
  );
}

type Bounds = { x: number; y: number; width: number; height: number };
/** One persistent visual object; anchors are measured, never hard-coded to a window size. */
export const OnboardingBrandMotion = memo(function OnboardingBrandMotion({
  phase,
  onArrive,
}: {
  phase: "intro" | "header" | "app";
  onArrive?: () => void;
}) {
  const reduced = useReducedMotion();
  const [bounds, setBounds] = useState<Bounds | null>(null);
  const [appTarget, setAppTarget] = useState(false);
  const done = useRef(false);
  const onArriveRef = useRef(onArrive);
  onArriveRef.current = onArrive;
  useLayoutEffect(() => {
    let frame = 0;
    let observer: ResizeObserver | undefined;
    let resizeHandler: (() => void) | undefined;
    let cancelled = false;
    let attempts = 0;
    if (phase === "app")
      document.documentElement.dataset.onboardingEntering = "true";
    const settle = () => {
      if (!done.current) {
        done.current = true;
        onArriveRef.current?.();
      }
    };
    const measure = () => {
      if (cancelled) return;
      const candidates = Array.from(
        document.querySelectorAll<HTMLElement>(
          phase === "app"
            ? "[data-onboarding-brand-target], .chat-welcome-mark"
            : `[data-onboarding-brand-anchor="${phase}"]`,
        ),
      );
      const anchor = candidates.find((element) => {
        if (element.closest(".onboarding-brand-motion")) return false;
        const r = element.getBoundingClientRect();
        return (
          r.width > 0 &&
          r.height > 0 &&
          r.right > 0 &&
          r.left < innerWidth &&
          r.bottom > 0 &&
          r.top < innerHeight
        );
      });
      if (!anchor) {
        if (++attempts < 45) frame = requestAnimationFrame(measure);
        else if (phase === "app") settle();
        return;
      }
      const update = () => {
        const r = anchor.getBoundingClientRect();
        const width = phase === "app" ? r.width / 0.68 : r.width;
        const height = phase === "app" ? r.height / 0.68 : r.height;
        setBounds({
          x: r.x - (width - r.width) / 2,
          y: r.y - (height - r.height) / 2,
          width,
          height,
        });
      };
      update();
      if (phase === "app") setAppTarget(true);
      observer = new ResizeObserver(update);
      observer.observe(anchor);
      resizeHandler = update;
      window.addEventListener("resize", update);
    };
    frame = requestAnimationFrame(measure);
    // Finish even if the OS/browser suppresses animation-completion events.
    const timer = phase === "app" ? setTimeout(settle, 1000) : undefined;
    return () => {
      cancelled = true;
      cancelAnimationFrame(frame);
      observer?.disconnect();
      if (resizeHandler) window.removeEventListener("resize", resizeHandler);
      clearTimeout(timer);
      delete document.documentElement.dataset.onboardingEntering;
    };
  }, [phase]);
  if (!bounds) return null;
  const arrive = () => {
    if (phase === "app" && appTarget && !done.current) {
      done.current = true;
      onArriveRef.current?.();
    }
  };
  return createPortal(
    <motion.div
      className="onboarding-brand-motion"
      data-phase={phase}
      aria-hidden
      initial={false}
      animate={{ ...bounds, opacity: phase === "app" && reduced ? 0 : 1 }}
      transition={
        reduced
          ? { duration: 0, opacity: { duration: 0.16 } }
          : { type: "spring", bounce: 0, duration: 0.46 }
      }
      onAnimationComplete={arrive}
    >
      <OnboardingLogo compact={phase !== "intro"} />
    </motion.div>,
    document.body,
  );
});
