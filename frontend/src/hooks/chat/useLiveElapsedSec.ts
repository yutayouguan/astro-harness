import { useEffect, useState } from "react";
import { elapsedSecSince } from "../../lib/chat/elapsedSec";

/**
 * 流式进行中按 ~100ms 刷新已过秒数；非 active 时返回 null。
 * startedAtMs 变化时立即对齐显示。
 */
export function useLiveElapsedSec(
  active: boolean,
  startedAtMs?: number | null,
): number | null {
  const [elapsed, setElapsed] = useState<number | null>(null);

  useEffect(() => {
    if (!active || startedAtMs == null) {
      setElapsed(null);
      return;
    }
    const tick = () => setElapsed(elapsedSecSince(startedAtMs));
    tick();
    const id = window.setInterval(tick, 1000);
    return () => window.clearInterval(id);
  }, [active, startedAtMs]);

  return elapsed;
}
