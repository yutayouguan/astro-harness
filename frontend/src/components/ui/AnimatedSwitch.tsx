/** 动画开关控件。 */
import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";

const EXIT_MS = 140;
const ENTER_MS = 220;

function usePrefersReducedMotion(): boolean {
  const [reduced, setReduced] = useState(() => {
    if (typeof window === "undefined") return false;
    return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  });

  useEffect(() => {
    const mq = window.matchMedia("(prefers-reduced-motion: reduce)");
    const onChange = () => setReduced(mq.matches);
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, []);

  return reduced;
}

type Phase = "idle" | "exit" | "enter";

/** 按 key 切换子树时的过渡动画入参 */
type Props = {
  /** 变化时触发切换动画 */
  switchKey: string | number | boolean;
  children: ReactNode;
  className?: string;
  /** soft = 淡入淡出 + 轻微位移；fade = 仅透明度 */
  variant?: "soft" | "fade";
  /**
   * out-in：先淡出旧内容再换入（默认）
   * enter：立刻换内容并淡入，避免 Tab 已切、旧内容还在淡出
   */
  mode?: "out-in" | "enter";
};

/**
 * 轻量内容切换：先短促淡出，再淡入新内容。
 * 无 framer-motion 依赖，尊重 prefers-reduced-motion。
 */
export default function AnimatedSwitch({
  switchKey,
  children,
  className = "",
  variant = "soft",
  mode = "out-in",
}: Props) {
  const reduceMotion = usePrefersReducedMotion();
  const [shown, setShown] = useState(children);
  const [shownKey, setShownKey] = useState(switchKey);
  const [phase, setPhase] = useState<Phase>("idle");
  const childrenRef = useRef(children);
  const pendingKeyRef = useRef(switchKey);
  const phaseRef = useRef<Phase>(phase);

  childrenRef.current = children;
  phaseRef.current = phase;

  // 同 key 时同步最新 children（列表刷新等）
  useLayoutEffect(() => {
    if (switchKey === shownKey && phaseRef.current !== "exit") {
      setShown(children);
    }
  }, [children, switchKey, shownKey]);

  useEffect(() => {
    if (switchKey === shownKey) return;

    pendingKeyRef.current = switchKey;

    if (reduceMotion) {
      setShownKey(switchKey);
      setShown(childrenRef.current);
      setPhase("idle");
      return;
    }

    if (mode === "enter") {
      setShownKey(switchKey);
      setShown(childrenRef.current);
      setPhase("enter");
      return;
    }

    if (phaseRef.current === "exit") return;
    setPhase("exit");
  }, [switchKey, shownKey, reduceMotion, mode]);

  useEffect(() => {
    if (phase !== "exit") return;
    const id = window.setTimeout(() => {
      setShownKey(pendingKeyRef.current);
      setShown(childrenRef.current);
      setPhase("enter");
    }, EXIT_MS);
    return () => window.clearTimeout(id);
  }, [phase]);

  useEffect(() => {
    if (phase !== "enter") return;
    const id = window.setTimeout(() => setPhase("idle"), ENTER_MS);
    return () => window.clearTimeout(id);
  }, [phase]);

  const cls = [
    "anim-switch",
    `anim-switch--${variant}`,
    phase === "enter" && "is-enter",
    phase === "exit" && "is-exit",
    className,
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <div className={cls} data-anim-key={String(shownKey)}>
      {shown}
    </div>
  );
}
