import {
  memo,
  useCallback,
  useEffect,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import { ONBOARDING_WARP_MS } from "../../lib/ui/onboardingMotion";

const RAYS = Array.from({ length: 40 }, (_, index) => ({
  angle: index * 9 + (index % 3) * 1.3,
  delay: (index % 5) * 0.026,
  distance: 8 + (index % 7) * 2.4,
  length: 9 + (index % 5) * 3,
}));

/** One-shot, compositor-only decoration. It never captures input or drives setup state. */
export const OnboardingWarp = memo(function OnboardingWarp({
  mode,
  direction,
  reduced,
  onFinished,
}: {
  mode: "intro" | "step" | "app";
  direction: number;
  reduced: boolean;
  onFinished?: () => void;
}) {
  const [active, setActive] = useState(true);
  const finished = useRef(false);
  const callback = useRef(onFinished);
  callback.current = onFinished;
  const duration = reduced
    ? ONBOARDING_WARP_MS.reduced
    : ONBOARDING_WARP_MS[mode];
  const finish = useCallback(() => {
    if (finished.current) return;
    finished.current = true;
    setActive(false);
    callback.current?.();
  }, []);
  useEffect(() => {
    // Animation events may be suppressed by a hidden WebView or OS motion settings.
    const timer = window.setTimeout(finish, duration + 120);
    return () => window.clearTimeout(timer);
  }, [duration, finish]);
  if (!active) return null;
  return (
    <div
      className="onboarding-warp"
      data-mode={mode}
      data-direction={direction >= 0 ? "forward" : "backward"}
      aria-hidden
      style={{ "--warp-duration": `${duration}ms` } as CSSProperties}
      onAnimationEnd={(event) => {
        if (event.target === event.currentTarget) finish();
      }}
    >
      {!reduced && (
        <>
          <div className="onboarding-warp-core" />
          <div className="onboarding-warp-rings">
            {[0, 1, 2, 3, 4].map((index) => (
              <i
                key={index}
                style={{ "--ring-index": index } as CSSProperties}
              />
            ))}
          </div>
          <div className="onboarding-warp-rays">
            {RAYS.map((ray, index) => (
              <span
                key={index}
                style={
                  {
                    "--ray-angle": `${ray.angle}deg`,
                    "--ray-delay": ray.delay,
                    "--ray-distance": `${ray.distance}vmax`,
                    "--ray-length": `${ray.length}vmax`,
                  } as CSSProperties
                }
              >
                <i />
              </span>
            ))}
          </div>
        </>
      )}
    </div>
  );
});
