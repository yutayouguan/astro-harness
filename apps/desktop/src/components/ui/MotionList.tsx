import { Children, type ReactNode } from "react";
import { motion, AnimatePresence } from "framer-motion";

type Props = {
  children: ReactNode;
  className?: string;
  staggerMs?: number;
};

const reducedMotion =
  typeof window !== "undefined" &&
  window.matchMedia("(prefers-reduced-motion: reduce)").matches;

const ITEM_VARIANTS = {
  hidden: { opacity: 0, y: 6 },
  visible: (i: number) => ({
    opacity: 1,
    y: 0,
    transition: {
      type: "spring" as const,
      stiffness: 420,
      damping: 32,
      mass: 0.7,
      delay: i * 0.032,
    },
  }),
  exit: {
    opacity: 0,
    y: -4,
    transition: { duration: 0.12, ease: "easeOut" as const },
  },
};

export default function MotionList({
  children,
  className = "",
  staggerMs = 32,
}: Props) {
  if (reducedMotion) {
    return <div className={className}>{children}</div>;
  }

  const items = Children.toArray(children);

  return (
    <div className={className}>
      <AnimatePresence initial={false}>
        {items.map((child, i) => (
          <motion.div
            key={(child as { key?: string }).key ?? i}
            custom={i}
            variants={{
              ...ITEM_VARIANTS,
              visible: (idx: number) => ({
                opacity: 1,
                y: 0,
                transition: {
                  type: "spring",
                  stiffness: 420,
                  damping: 32,
                  mass: 0.7,
                  delay: idx * (staggerMs / 1000),
                },
              }),
            }}
            initial="hidden"
            animate="visible"
            exit="exit"
          >
            {child}
          </motion.div>
        ))}
      </AnimatePresence>
    </div>
  );
}
