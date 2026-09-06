import { useRef, type ReactNode } from "react";
import {
  AnimatePresence,
  motion,
  type Transition,
  type TargetAndTransition,
} from "framer-motion";

type Variant = "soft" | "fade" | "slide-left" | "slide-right";
type Mode = "wait" | "popLayout";

const SPRING: Transition = {
  type: "spring",
  stiffness: 380,
  damping: 30,
  mass: 0.8,
};

const FAST: Transition = {
  type: "spring",
  stiffness: 500,
  damping: 35,
  mass: 0.6,
};

const VARIANTS: Record<
  Variant,
  {
    initial: TargetAndTransition;
    animate: TargetAndTransition;
    exit: TargetAndTransition;
    transition: Transition;
  }
> = {
  soft: {
    initial: { opacity: 0, y: 8, scale: 0.985 },
    animate: { opacity: 1, y: 0, scale: 1 },
    exit: { opacity: 0, y: -6, scale: 0.99 },
    transition: SPRING,
  },
  fade: {
    initial: { opacity: 0 },
    animate: { opacity: 1 },
    exit: { opacity: 0 },
    transition: { duration: 0.16, ease: "easeOut" },
  },
  "slide-left": {
    initial: { opacity: 0, x: 24 },
    animate: { opacity: 1, x: 0 },
    exit: { opacity: 0, x: -24 },
    transition: FAST,
  },
  "slide-right": {
    initial: { opacity: 0, x: -24 },
    animate: { opacity: 1, x: 0 },
    exit: { opacity: 0, x: 24 },
    transition: FAST,
  },
};

type Props = {
  switchKey: string | number | boolean;
  children: ReactNode;
  className?: string;
  variant?: Variant;
  mode?: Mode;
};

export default function MotionSwitch({
  switchKey,
  children,
  className = "",
  variant = "soft",
  mode = "wait",
}: Props) {
  const reducedMotion =
    typeof window !== "undefined" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  const v = VARIANTS[variant];
  const key = String(switchKey);
  const prevKeyRef = useRef(key);

  if (reducedMotion) {
    return (
      <div className={`anim-switch anim-switch--${variant} ${className}`}>
        {children}
      </div>
    );
  }

  const direction =
    variant === "soft" || variant === "fade"
      ? undefined
      : key > prevKeyRef.current
        ? 1
        : -1;

  prevKeyRef.current = key;

  return (
    <AnimatePresence mode={mode} initial={false}>
      <motion.div
        key={key}
        className={`anim-switch anim-switch--${variant} ${className}`}
        initial={
          direction !== undefined
            ? ({ opacity: 0, x: direction * 24 } as TargetAndTransition)
            : v.initial
        }
        animate={v.animate}
        exit={
          direction !== undefined
            ? ({ opacity: 0, x: direction * -24 } as TargetAndTransition)
            : v.exit
        }
        transition={v.transition}
      >
        {children}
      </motion.div>
    </AnimatePresence>
  );
}
