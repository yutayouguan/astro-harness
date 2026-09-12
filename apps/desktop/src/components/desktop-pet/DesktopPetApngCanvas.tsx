import { useEffect, useRef, useState } from "react";
import { apngFrameAt, decodePetApng, type PetApng } from "../../lib/ui/petApng";
import { resolveMediaSrc } from "../../lib/media/resolveMediaSrc";
import {
  advancePetGaze,
  shortestAngleDelta,
  stablePetGazeFrame,
} from "../../lib/ui/desktopPetMotion";
import type { DesktopPetCanvasProps } from "./DesktopPetCanvas";

/** Decode once per action, paint deterministic frames. No autonomous <img> clock:
 * pause/reduced motion and alpha hit testing must observe the exact same frame. */
export default function DesktopPetApngCanvas(props: DesktopPetCanvasProps) {
  const {
    src,
    motionClips = {},
    state,
    motionName,
    lookAngle,
    repeatMotion = false,
    reducedMotion = false,
    className,
    label = "Animated desktop pet",
  } = props;
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const targetAngle = useRef(lookAngle);
  targetAngle.current = lookAngle;
  const cache = useRef(new Map<string, PetApng>());
  const [error, setError] = useState(false);
  const name = motionName ?? state;
  const spec = motionClips[name] ?? motionClips.idle;
  const path = spec?.path ?? src;
  const timings = JSON.stringify(spec?.durationsMs ?? []);

  useEffect(() => {
    cache.current.clear();
    const canvas = canvasRef.current;
    const context = canvas?.getContext("2d");
    context?.setTransform(1, 0, 0, 1, 0, 0);
    if (canvas) context?.clearRect(0, 0, canvas.width, canvas.height);
  }, [src]);
  useEffect(() => {
    const abort = new AbortController();
    let disposed = false,
      request = 0;
    setError(false);
    const canvas = canvasRef.current;
    // Same pet: hold the last pose until the next action is ready, no blank flash.
    // Identity changes clear the canvas in the separate src effect above.
    const start = async () => {
      let decoded = cache.current.get(path);
      if (!decoded) {
        const response = await fetch(resolveMediaSrc(path) || path, {
          signal: abort.signal,
        });
        if (!response.ok) throw new Error("APNG unavailable");
        decoded = await decodePetApng(await response.arrayBuffer());
      }
      if (disposed || !canvas) return;
      const expected: number[] = JSON.parse(timings);
      if (
        expected.length &&
        (expected.length !== decoded.frames.length ||
          expected.some(
            (time, i) => Math.abs(time - decoded!.durations[i]) > 1,
          ))
      )
        throw new Error("APNG timing does not match manifest");
      cache.current.set(path, decoded);
      // A pet library can contain many clips; don't preload every decoded action.
      while (cache.current.size > 3)
        cache.current.delete(cache.current.keys().next().value!);
      let started: number | null = null,
        lastTime = 0,
        painted = -1;
      let gaze = targetAngle.current ?? 0,
        lastGaze: number | null = null;
      const draw = (now: number) => {
        if (disposed) return;
        started ??= now;
        const looking =
          !motionName && state === "look" && targetAngle.current != null;
        const sampled = reducedMotion
          ? { index: 0, done: true }
          : apngFrameAt(
              decoded!.durations,
              now - started,
              motionName ? repeatMotion : !looking,
            );
        let moving = false;
        if (looking && !reducedMotion) {
          gaze = advancePetGaze(
            gaze,
            targetAngle.current!,
            lastTime ? now - lastTime : 16,
          );
          sampled.index = stablePetGazeFrame(gaze, lastGaze);
          lastGaze = sampled.index;
          moving =
            Math.abs(shortestAngleDelta(gaze, targetAngle.current!)) > 0.3;
        }
        lastTime = now;
        const ratio = Math.min(window.devicePixelRatio || 1, 2);
        const resized =
          canvas.width !== Math.round(192 * ratio) ||
          canvas.height !== Math.round(208 * ratio);
        if (resized) {
          canvas.width = Math.round(192 * ratio);
          canvas.height = Math.round(208 * ratio);
        }
        if (painted !== sampled.index || resized) {
          const context = canvas.getContext("2d", { willReadFrequently: true });
          if (!context) return;
          context.setTransform(ratio, 0, 0, ratio, 0, 0);
          context.clearRect(0, 0, 192, 208);
          context.drawImage(
            decoded!.frames[sampled.index] ?? decoded!.frames[0],
            0,
            0,
            192,
            208,
          );
          painted = sampled.index;
        }
        // Keep gaze responsive to its ref without restarting the APNG on pointer updates.
        if (!reducedMotion && (looking || moving || !sampled.done))
          request = requestAnimationFrame(draw);
      };
      request = requestAnimationFrame(draw);
    };
    void start().catch(() => {
      if (!disposed) setError(true);
    });
    return () => {
      disposed = true;
      abort.abort();
      cancelAnimationFrame(request);
    };
  }, [src, path, timings, state, motionName, repeatMotion, reducedMotion]);

  return (
    <>
      <canvas
        ref={canvasRef}
        className={className}
        role="img"
        aria-label={label}
      />
      {error && (
        <span className="desktop-pet-image-error" role="alert">
          {label}: APNG 无法加载 / APNG unavailable
        </span>
      )}
    </>
  );
}
