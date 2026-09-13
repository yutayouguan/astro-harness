import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { apngFrameAt } from "../../lib/ui/petApng";
import { acquirePetApng } from "../../lib/ui/petApngCache";
import { createPetPlaybackClock } from "../../lib/ui/petPlaybackClock";
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
    paused = false,
    startAtMs = 0,
    className,
    label = "Animated desktop pet",
  } = props;
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const targetAngle = useRef(lookAngle);
  targetAngle.current = lookAngle;
  const pausedRef = useRef(paused);
  pausedRef.current = paused;
  const redraw = useRef<(() => void) | null>(null);
  const onMotionEnd = useRef(props.onMotionEnd);
  onMotionEnd.current = props.onMotionEnd;
  const onReady = useRef(props.onReady); onReady.current = props.onReady;
  const onLoadError = useRef(props.onLoadError); onLoadError.current = props.onLoadError;
  const externalElapsed = useRef(props.externalElapsedMs);
  externalElapsed.current = props.externalElapsedMs;
  const [onScreen, setOnScreen] = useState(false);
  const [documentVisible, setDocumentVisible] = useState(!document.hidden);
  const [error, setError] = useState(false);
  const [shape, setShape] = useState({ width: 192, height: 208 });
  const name = motionName ?? state;
  const spec = motionClips[name] ?? motionClips.idle;
  const path = spec?.path ?? src;
  const signature = JSON.stringify(spec ?? null);
  const playbackClock = useRef(createPetPlaybackClock());
  useLayoutEffect(() => {
    playbackClock.current.reset();
  }, [src, path, signature, state, motionName, repeatMotion, reducedMotion, startAtMs]);

  useEffect(() => {
    const observer = new IntersectionObserver((entries) =>
      setOnScreen(entries.some((entry) => entry.isIntersecting)),
    );
    if (canvasRef.current) observer.observe(canvasRef.current);
    const visibility = () => setDocumentVisible(!document.hidden);
    const resize = () => redraw.current?.();
    document.addEventListener("visibilitychange", visibility);
    window.addEventListener("resize", resize);
    return () => {
      observer.disconnect();
      document.removeEventListener("visibilitychange", visibility);
      window.removeEventListener("resize", resize);
    };
  }, []);
  useLayoutEffect(() => {
    redraw.current?.();
  }, [paused, lookAngle, props.externalElapsedMs]);

  useEffect(() => {
    const canvas = canvasRef.current;
    const context = canvas?.getContext("2d");
    context?.setTransform(1, 0, 0, 1, 0, 0);
    if (canvas) context?.clearRect(0, 0, canvas.width, canvas.height);
  }, [src]);
  useEffect(() => {
    if (!onScreen || !documentVisible) return;
    let disposed = false,
      request = 0;
    let wakeTimer = 0;
    setError(false);
    const canvas = canvasRef.current;
    const lease = acquirePetApng(resolveMediaSrc(path) || path, reducedMotion);
    // Same pet: hold the last pose until the next action is ready, no blank flash.
    // Identity changes clear the canvas in the separate src effect above.
    const start = async () => {
      const decoded = await lease.ready;
      if (disposed || !canvas) return;
      const clip = JSON.parse(signature) as typeof spec;
      const expected = clip?.durationsMs ?? [];
      if (
        (clip && (clip.frameWidth !== decoded.width || clip.frameHeight !== decoded.height)) ||
        (expected.length &&
        (expected.length !== decoded.frameCount ||
          expected.some(
            (time, i) => Math.abs(time - decoded!.durations[i]) > 1,
          )))
      )
        throw new Error("APNG timing does not match manifest");
      setShape({ width: decoded.width, height: decoded.height });
      const clock = playbackClock.current;
      let lastTime = 0,
        painted = -1;
      let ended = false;
      let ready = false;
      let gaze = targetAngle.current ?? 0,
        lastGaze: number | null = null;
      const draw = (now: number) => {
        request = 0;
        if (disposed) return;
        const elapsed =
          externalElapsed.current ??
          (startAtMs + clock.sample(now, pausedRef.current || reducedMotion));
        const looking =
          !motionName && state === "look" && targetAngle.current != null;
        const sampled = reducedMotion
          ? { index: 0, done: true, waitMs: Infinity }
          : apngFrameAt(
              decoded!.durations,
              elapsed,
              motionName ? repeatMotion : false,
              clip,
              !motionName && ["idle", "running", "waiting"].includes(state),
            );
        let moving = false;
        if (looking && !reducedMotion) {
          if (!pausedRef.current)
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
          canvas.width !== Math.round(decoded.width * ratio) ||
          canvas.height !== Math.round(decoded.height * ratio);
        if (resized) {
          canvas.width = Math.round(decoded.width * ratio);
          canvas.height = Math.round(decoded.height * ratio);
        }
        if (painted !== sampled.index || resized) {
          const context = canvas.getContext("2d", { willReadFrequently: true });
          if (!context) return;
          context.setTransform(ratio, 0, 0, ratio, 0, 0);
          context.clearRect(0, 0, decoded.width, decoded.height);
          context.drawImage(
            decoded!.frames[sampled.index] ?? decoded!.frames[0],
            0,
            0,
            decoded.width,
            decoded.height,
          );
          painted = sampled.index;
          if (!ready) { ready = true; onReady.current?.(); }
        }
        if (
          sampled.done &&
          motionName &&
          !repeatMotion &&
          !pausedRef.current &&
          !reducedMotion &&
          !ended
        ) {
          ended = true;
          onMotionEnd.current?.();
        }
        // Keep gaze responsive to its ref without restarting the APNG on pointer updates.
        if (
          externalElapsed.current === undefined &&
          !reducedMotion &&
          !pausedRef.current &&
          (moving || (!looking && !sampled.done))
        ) {
          if (moving) request = requestAnimationFrame(draw);
          else wakeTimer = window.setTimeout(() => { wakeTimer = 0; if (!disposed) request = requestAnimationFrame(draw); }, Math.max(0, sampled.waitMs - 2));
        }
      };
      redraw.current = () => {
        clearTimeout(wakeTimer); wakeTimer = 0;
        if (!request && !disposed) request = requestAnimationFrame(draw);
      };
      request = requestAnimationFrame(draw);
    };
    void start().catch(() => {
      if (!disposed) { setError(true); onLoadError.current?.(); }
    });
    return () => {
      disposed = true;
      playbackClock.current.sample(performance.now(), true);
      redraw.current = null;
      lease.release();
      cancelAnimationFrame(request);
      clearTimeout(wakeTimer);
    };
  }, [
    src,
    path,
    signature,
    state,
    motionName,
    repeatMotion,
    reducedMotion,
    onScreen,
    documentVisible,
    startAtMs,
  ]);

  return (
    <>
      <canvas
        ref={canvasRef}
        className={className}
        style={{ aspectRatio: `${shape.width} / ${shape.height}` }}
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
