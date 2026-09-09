/** One timing contract for the portal, stage changes, and App handoff. */
export const ONBOARDING_WARP_MS = {
  intro: 2200,
  step: 560,
  app: 980,
  reduced: 160,
} as const;

const EASE = [0.22, 0.8, 0.24, 1] as const;
const depth = (z: number) => `perspective(1200px) translateZ(${z}px)`;

export function onboardingStageVariants(reduced: boolean) {
  if (reduced) {
    return {
      enter: { opacity: 0 },
      center: { opacity: 1, transition: { duration: 0.12 } },
      exit: { opacity: 0, transition: { duration: 0.08 } },
    };
  }
  return {
    enter: (direction: number) => ({
      opacity: 0,
      transform: depth(direction >= 0 ? -150 : 150),
    }),
    center: {
      opacity: 1,
      transform: depth(0),
      transition: { duration: 0.3, ease: EASE },
    },
    // AnimatePresence passes the latest direction, including when reversing a step.
    exit: (direction: number) => ({
      opacity: 0,
      transform: depth(direction >= 0 ? 150 : -150),
      transition: { duration: 0.18, ease: EASE },
    }),
  };
}
