/** 空会话输入框：循环打字 / 删除的建议话术。 */
import { useEffect, useMemo, useState } from "react";

type Options = {
  /** 每个字符打出间隔 */
  typeMs?: number;
  /** 每个字符删除间隔 */
  deleteMs?: number;
  /** 打完后停顿 */
  holdMs?: number;
  /** 清空后切换下一条前停顿 */
  gapMs?: number;
};

/**
 * `enabled` 为 false 时返回空串（由调用方改用静态 placeholder）。
 * 尊重 `prefers-reduced-motion`：减少动效时整句轮播、不逐字打字。
 */
export function useTypingPlaceholder(
  phrases: string[],
  enabled: boolean,
  opts: Options = {},
): string {
  const typeMs = opts.typeMs ?? 46;
  const deleteMs = opts.deleteMs ?? 28;
  const holdMs = opts.holdMs ?? 1800;
  const gapMs = opts.gapMs ?? 420;
  const phraseKey = useMemo(
    () => phrases.map((p) => p.trim()).filter(Boolean).join("\0"),
    [phrases],
  );

  const [text, setText] = useState("");

  useEffect(() => {
    if (!enabled) {
      setText("");
      return;
    }

    const clean = phraseKey ? phraseKey.split("\0") : [];
    if (!clean.length) {
      setText("");
      return;
    }

    const reduceMotion =
      typeof window !== "undefined" &&
      window.matchMedia("(prefers-reduced-motion: reduce)").matches;

    let cancelled = false;
    let timer = 0;
    let index = 0;
    let phase: "type" | "hold" | "delete" | "gap" = "type";
    let cursor = 0;

    const schedule = (fn: () => void, ms: number) => {
      timer = window.setTimeout(() => {
        if (!cancelled) fn();
      }, ms);
    };

    const tick = () => {
      if (cancelled) return;
      const full = clean[index] ?? clean[0];

      if (reduceMotion) {
        setText(full);
        schedule(() => {
          index = (index + 1) % clean.length;
          tick();
        }, holdMs + gapMs);
        return;
      }

      if (phase === "type") {
        cursor = Math.min(full.length, cursor + 1);
        setText(full.slice(0, cursor));
        if (cursor >= full.length) {
          phase = "hold";
          schedule(tick, holdMs);
        } else {
          schedule(tick, typeMs);
        }
        return;
      }

      if (phase === "hold") {
        phase = "delete";
        schedule(tick, deleteMs);
        return;
      }

      if (phase === "delete") {
        cursor = Math.max(0, cursor - 1);
        setText(full.slice(0, cursor));
        if (cursor <= 0) {
          phase = "gap";
          schedule(tick, gapMs);
        } else {
          schedule(tick, deleteMs);
        }
        return;
      }

      index = (index + 1) % clean.length;
      phase = "type";
      cursor = 0;
      schedule(tick, typeMs);
    };

    tick();

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [enabled, phraseKey, typeMs, deleteMs, holdMs, gapMs]);

  return text;
}
