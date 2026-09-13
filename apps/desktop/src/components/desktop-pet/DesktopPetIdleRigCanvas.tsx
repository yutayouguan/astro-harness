import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { probeIdleRig, type IdleRig } from "../../lib/ui/petIdleRig";
import {
  advanceRigGaze,
  idleRigPose,
  idleRigWakeMs,
  restingGaze,
} from "../../lib/ui/petIdleRigMotion";
import { createPetPlaybackClock } from "../../lib/ui/petPlaybackClock";
import type { DesktopPetCanvasProps } from "./DesktopPetCanvas";

export default function DesktopPetIdleRigCanvas(
  props: DesktopPetCanvasProps & { fallback: ReactNode },
) {
  const [loaded, setLoaded] = useState<{ src: string; rig: IdleRig } | null>(
    null,
  );
  const [systemReduced, setSystemReduced] = useState(
    () => window.matchMedia("(prefers-reduced-motion: reduce)").matches,
  );
  const current = useRef(props);
  current.current = {
    ...props,
    reducedMotion: props.reducedMotion || systemReduced,
  };
  useEffect(() => {
    const query = window.matchMedia("(prefers-reduced-motion: reduce)");
    const changed = () => setSystemReduced(query.matches);
    query.addEventListener("change", changed);
    return () => query.removeEventListener("change", changed);
  }, []);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const activatedSource = useRef<string | null>(null);
  const redraw = useRef<(() => void) | null>(null);
  useEffect(() => {
    let cancelled = false;
    void probeIdleRig(props.src).then((rig) => {
      if (!cancelled) setLoaded(rig ? { src: props.src, rig } : null);
    });
    return () => {
      cancelled = true;
    };
  }, [props.src]);
  // A late layer load must not replace a pose the user has already paused.
  const rig =
    loaded?.src === props.src &&
    (!props.paused || activatedSource.current === props.src)
      ? loaded.rig
      : null;
  const width =
    rig?.canvasWidth ??
    (/\.apng(?:[?#]|$)/i.test(props.src)
      ? (props.motionClips?.idle?.frameWidth ?? 192)
      : 192);
  useLayoutEffect(() => {
    redraw.current?.();
  }, [props.paused, props.reducedMotion, props.lookAngle, systemReduced]);
  useLayoutEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !rig) return;
    const head = document.createElement("canvas");
    head.width = 192;
    head.height = 208;
    const headContext = head.getContext("2d")!;
    const context = canvas.getContext("2d", { willReadFrequently: true })!;
    const clock = createPetPlaybackClock();
    let gaze = restingGaze(),
      request = 0,
      timer = 0,
      visible = false,
      last = 0,
      painted = "",
      disposed = false,
      ready = false;
    const { config, images } = rig;
    activatedSource.current = props.src;
    // The replacement canvas must contain a neutral frame before browser paint;
    // waiting for IntersectionObserver would flash an empty default-sized canvas.
    const initialRatio = Math.min(devicePixelRatio || 1, 2);
    canvas.width = Math.round(width * initialRatio);
    canvas.height = Math.round(208 * initialRatio);
    context.setTransform(initialRatio, 0, 0, initialRatio, 0, 0);
    context.drawImage(images.neutral, (width - 192) / 2, 0);
    ready = true;
    current.current.onReady?.();
    function draw(now: number) {
      request = 0;
      if (disposed) return;
      const p = current.current;
      const paused = p.paused || p.reducedMotion || !visible || document.hidden;
      const elapsed = clock.sample(now, !!paused);
      if (!visible || document.hidden) return;
      const previousGaze = gaze;
      if (p.reducedMotion) gaze = restingGaze();
      else if (!paused)
        gaze = advanceRigGaze(gaze, p.lookAngle, last ? now - last : 16);
      last = now;
      const pose = p.reducedMotion
        ? { blink: 0, ear: 0, tail: 0 }
        : idleRigPose(elapsed + 400, config.tailDegrees);
      const ratio = Math.min(devicePixelRatio || 1, 2);
      const pixelWidth = Math.round(width * ratio),
        pixelHeight = Math.round(208 * ratio);
      const signature = JSON.stringify([
        pose.blink,
        ...[pose.ear, pose.tail, ...Object.values(gaze)].map((v) =>
          Math.round(v * 50),
        ),
        ratio,
      ]);
      if (
        painted !== signature ||
        canvas!.width !== pixelWidth ||
        canvas!.height !== pixelHeight
      ) {
        if (canvas!.width !== pixelWidth || canvas!.height !== pixelHeight) {
          canvas!.width = pixelWidth;
          canvas!.height = pixelHeight;
        }
        context.setTransform(ratio, 0, 0, ratio, 0, 0);
        context.clearRect(0, 0, width, 208);
        const padding = (width - 192) / 2;
        context.save();
        context.translate(padding, 0);
        if (
          p.reducedMotion ||
          (pose.blink === 0 &&
            pose.tail === 0 &&
            pose.ear === 0 &&
            Object.values(gaze).every((v) => v === 0))
        ) {
          context.drawImage(images.neutral, 0, 0);
        } else {
          context.save();
          context.translate(...(config.tailPivot as [number, number]));
          context.rotate((pose.tail * Math.PI) / 180);
          context.translate(-config.tailPivot[0], -config.tailPivot[1]);
          context.drawImage(images.tail, 0, 0);
          context.restore();
          context.drawImage(images.body, 0, 0);
          headContext.clearRect(0, 0, 192, 208);
          headContext.drawImage(
            images.heads,
            pose.blink * 192,
            0,
            192,
            208,
            0,
            0,
            192,
            208,
          );
          // Move eye textures within their own apertures, not the whole portrait.
          for (const eye of config.eyes) {
            headContext.save();
            headContext.beginPath();
            headContext.ellipse(
              eye.x,
              eye.y,
              eye.rx,
              eye.ry,
              0,
              0,
              Math.PI * 2,
            );
            headContext.clip();
            headContext.drawImage(
              images.heads,
              pose.blink * 192,
              0,
              192,
              208,
              gaze.eyeX,
              gaze.eyeY,
              192,
              208,
            );
            headContext.restore();
          }
          // A tiny, occasional local ear texture flex; the ear silhouette stays intact.
          for (const [x, y, rx, ry] of config.ears) {
            headContext.save();
            headContext.beginPath();
            headContext.ellipse(x, y, rx, ry, 0, 0, Math.PI * 2);
            headContext.clip();
            headContext.drawImage(
              images.heads,
              pose.blink * 192,
              0,
              192,
              208,
              pose.ear * 0.35,
              0,
              192,
              208,
            );
            headContext.restore();
          }
          context.drawImage(head, gaze.headX, gaze.headY);
        }
        context.restore();
        painted = signature;
        if (!ready) {
          ready = true;
          current.current.onReady?.();
        }
      }
      const gazeMoving = Object.keys(gaze).some(
        (key) =>
          gaze[key as keyof typeof gaze] !==
          previousGaze[key as keyof typeof gaze],
      );
      if (!paused)
        timer = window.setTimeout(
          wake,
          gazeMoving ? 32 : idleRigWakeMs(elapsed + 400),
        );
    }
    function wake() {
      clearTimeout(timer);
      if (!request && !disposed) request = requestAnimationFrame(draw);
    }
    redraw.current = wake;
    const observer = new IntersectionObserver((entries) => {
      visible = entries.some((e) => e.isIntersecting);
      if (!visible) {
        clock.sample(performance.now(), true);
        last = 0;
      }
      wake();
    });
    observer.observe(canvas);
    const visibilityChanged = () => {
      if (document.hidden) {
        clock.sample(performance.now(), true);
        last = 0;
      }
      wake();
    };
    document.addEventListener("visibilitychange", visibilityChanged);
    window.addEventListener("resize", wake);
    return () => {
      disposed = true;
      observer.disconnect();
      clearTimeout(timer);
      cancelAnimationFrame(request);
      redraw.current = null;
      document.removeEventListener("visibilitychange", visibilityChanged);
      window.removeEventListener("resize", wake);
    };
  }, [rig, width]);
  return rig ? (
    <canvas
      ref={canvasRef}
      className={props.className}
      style={{ aspectRatio: `${width} / 208` }}
      role="img"
      aria-label={props.label ?? "Animated desktop pet"}
      data-pet-renderer="layered-idle"
    />
  ) : (
    props.fallback
  );
}
