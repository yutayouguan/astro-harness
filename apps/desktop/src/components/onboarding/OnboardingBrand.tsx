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
      {compact ? (
        <AstroLogoMark className="onboarding-logo-base" />
      ) : (
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
}: {
  phase: "intro" | "header";
}) {
  const reduced = useReducedMotion();
  const [bounds, setBounds] = useState<Bounds | null>(null);
  const [wordmarkBounds, setWordmarkBounds] = useState<
    (Bounds & { fontSize: number }) | null
  >(null);
  useLayoutEffect(() => {
    let frame = 0;
    let observer: ResizeObserver | undefined;
    let resizeHandler: (() => void) | undefined;
    let cancelled = false;
    let attempts = 0;
    const measure = () => {
      if (cancelled) return;
      const candidates = Array.from(
        document.querySelectorAll<HTMLElement>(
          `[data-onboarding-brand-anchor="${phase}"]`,
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
        return;
      }
      const update = () => {
        const r = anchor.getBoundingClientRect();
        setBounds({
          x: r.x,
          y: r.y,
          width: r.width,
          height: r.height,
        });
        const wordmark = document.querySelector<HTMLElement>(
          `[data-onboarding-wordmark-anchor="${phase}"]`,
        );
        if (wordmark) {
          const textRect = wordmark.getBoundingClientRect();
          setWordmarkBounds({
            x: textRect.x,
            y: textRect.y,
            width: textRect.width,
            height: textRect.height,
            fontSize: parseFloat(getComputedStyle(wordmark).fontSize),
          });
        }
      };
      update();
      observer = new ResizeObserver(update);
      observer.observe(anchor);
      const wordmark = document.querySelector(
        `[data-onboarding-wordmark-anchor="${phase}"]`,
      );
      if (wordmark) observer.observe(wordmark);
      resizeHandler = update;
      window.addEventListener("resize", update);
    };
    frame = requestAnimationFrame(measure);
    return () => {
      cancelled = true;
      cancelAnimationFrame(frame);
      observer?.disconnect();
      if (resizeHandler) window.removeEventListener("resize", resizeHandler);
    };
  }, [phase]);
  if (!bounds) return null;
  const transition = reduced
    ? { duration: 0 }
    : { type: "spring" as const, bounce: 0, duration: 0.64 };
  return createPortal(
    <>
      <motion.div
        className="onboarding-brand-motion"
        data-phase={phase}
        aria-hidden
        initial={false}
        animate={bounds}
        transition={transition}
      >
        <OnboardingLogo compact={phase !== "intro"} />
      </motion.div>
      {wordmarkBounds && (
        <motion.span
          className="onboarding-brand-wordmark"
          data-phase={phase}
          aria-hidden
          initial={false}
          animate={wordmarkBounds}
          transition={transition}
        >
          Astro Harness
        </motion.span>
      )}
    </>,
    document.body,
  );
});
