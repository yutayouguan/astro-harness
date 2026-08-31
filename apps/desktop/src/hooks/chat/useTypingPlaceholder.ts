/** 空会话输入框：循环打字 / 删除的建议话术（直接写 DOM，避免高频 setState）。 */
import { useEffect, useMemo, useRef, type RefObject } from "react";

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
 * 将打字文案写入 `textRef` 指向的节点。
 * `enabled` 为 false 时清空；尊重 `prefers-reduced-motion`。
 */
export function useTypingPlaceholder(
  phrases: string[],
  enabled: boolean,
  textRef: RefObject<HTMLElement | null>,
  opts: Options = {},
): void {
  const typeMs = opts.typeMs ?? 46;
  const deleteMs = opts.deleteMs ?? 28;
  const holdMs = opts.holdMs ?? 1800;
  const gapMs = opts.gapMs ?? 420;
  const phraseKey = useMemo(
    () =>
      phrases
        .map((p) => p.trim())
        .filter(Boolean)
        .join("\0"),
    [phrases],
  );
  const optsRef = useRef({ typeMs, deleteMs, holdMs, gapMs });
  optsRef.current = { typeMs, deleteMs, holdMs, gapMs };

  useEffect(() => {
    const el = textRef.current;
    if (!el) return;

    const write = (value: string) => {
      if (textRef.current) textRef.current.textContent = value;
    };

    if (!enabled) {
      write("");
      return;
    }

    const clean = phraseKey ? phraseKey.split("\0") : [];
    if (!clean.length) {
      write("");
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
      window.clearTimeout(timer);
      timer = window.setTimeout(() => {
        if (!cancelled) fn();
      }, ms);
    };

    const tick = () => {
      if (cancelled) return;
      const {
        typeMs: tMs,
        deleteMs: dMs,
        holdMs: hMs,
        gapMs: gMs,
      } = optsRef.current;
      const full = clean[index] ?? clean[0];

      if (reduceMotion) {
        write(full);
        schedule(() => {
          index = (index + 1) % clean.length;
          tick();
        }, hMs + gMs);
        return;
      }

      if (phase === "type") {
        cursor = Math.min(full.length, cursor + 1);
        write(full.slice(0, cursor));
        if (cursor >= full.length) {
          phase = "hold";
          schedule(tick, hMs);
        } else {
          schedule(tick, tMs);
        }
        return;
      }

      if (phase === "hold") {
        phase = "delete";
        schedule(tick, dMs);
        return;
      }

      if (phase === "delete") {
        cursor = Math.max(0, cursor - 1);
        write(full.slice(0, cursor));
        if (cursor <= 0) {
          phase = "gap";
          schedule(tick, gMs);
        } else {
          schedule(tick, dMs);
        }
        return;
      }

      index = (index + 1) % clean.length;
      phase = "type";
      cursor = 0;
      schedule(tick, tMs);
    };

    tick();

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [enabled, phraseKey, textRef]);
}
