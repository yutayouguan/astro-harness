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
const INTRO_PARTICLES = Array.from({ length: 54 }, (_, index) => {
  return {
    x: 5 + (index % 9) * 11.2 + (index % 3) * 0.4,
    y: 6 + Math.floor(index / 9) * 17.4 + (index % 4) * 0.8,
    delay: (index % 7) * 34,
    size: 2 + (index % 3),
  };
});
/** Viewport-sized field, converging on the measured logo rather than a fixed point. */
export function OnboardingIntroParticles() {
  const field = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const anchor = document.querySelector(
      '[data-onboarding-brand-anchor="intro"]',
    );
    if (!anchor) return;
    const update = () => {
      const bounds = anchor.getBoundingClientRect();
      field.current?.style.setProperty(
        "--particle-origin-x",
        `${bounds.x + bounds.width / 2}px`,
      );
      field.current?.style.setProperty(
        "--particle-origin-y",
        `${bounds.y + bounds.height / 2}px`,
      );
    };
    update();
    const observer = new ResizeObserver(update);
    observer.observe(anchor);
    window.addEventListener("resize", update);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", update);
    };
  }, []);
  return (
    <div ref={field} className="onboarding-particle-field" aria-hidden>
      {INTRO_PARTICLES.map((particle, index) => (
        <i
          key={index}
          style={
            {
              "--particle-x": `${particle.x.toFixed(1)}vw`,
              "--particle-y": `${particle.y.toFixed(1)}dvh`,
              "--particle-delay": `${particle.delay}ms`,
              "--particle-size": `${particle.size}px`,
            } as CSSProperties
          }
        />
      ))}
    </div>
  );
}
export function OnboardingLogo({ compact = false }: { compact?: boolean }) {
  return (
    <div
      className={`onboarding-logo ${compact ? "is-compact" : ""}`}
      aria-hidden
    >
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
