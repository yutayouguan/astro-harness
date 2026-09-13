import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { loadPetAtlas } from "../../lib/ui/desktopPetAtlas";
import DesktopPetApngCanvas from "./DesktopPetApngCanvas";
import DesktopPetIdleRigCanvas from "./DesktopPetIdleRigCanvas";
import { usesIdleRig } from "../../lib/ui/petIdleRigMotion";
import { groomingFrame } from "../../lib/ui/desktopPetLeisure";
import {
  motionFrame,
  motionUsesNeutralFrame,
  type PetMotionClips,
} from "../../lib/ui/petMotionClip";
import { resolveMediaSrc } from "../../lib/media/resolveMediaSrc";
import {
  advancePetGaze,
  shortestAngleDelta,
  stablePetGazeFrame,
} from "../../lib/ui/desktopPetMotion";
import {
  DESKTOP_PET_CELL,
  frameForElapsed,
  type DesktopPetAnimationState,
} from "../../lib/ui/desktopPetAnimation";

export type DesktopPetCanvasProps = {
  src: string;
  groomingSrc?: string;
  motionClips?: PetMotionClips;
  motionName?: string;
  repeatMotion?: boolean;
  state: DesktopPetAnimationState;
  lookAngle?: number | null;
  className?: string;
  label?: string;
  reducedMotion?: boolean;
  paused?: boolean;
  onMotionEnd?: () => void;
  externalElapsedMs?: number;
  startAtMs?: number;
  onReady?: () => void;
  onLoadError?: () => void;
  clip?: "grooming";
};

export default function DesktopPetCanvas(props: DesktopPetCanvasProps) {
  const displayed = useRef(props);
  if (!props.paused || displayed.current.src !== props.src) displayed.current = props;
  const held = props.paused ? {
    ...props, state: displayed.current.state, motionName: displayed.current.motionName,
    motionClips: displayed.current.motionClips, lookAngle: displayed.current.lookAngle,
    clip: displayed.current.clip,
    externalElapsedMs: displayed.current.externalElapsedMs,
    startAtMs: displayed.current.startAtMs,
  } : props;
  const playback = /\.apng(?:[?#]|$)/i.test(props.src)
    ? <DesktopPetApngCanvas {...held} />
    : <LegacyDesktopPetCanvas {...held} />;
  return usesIdleRig(held.state, held.motionName, held.clip, held.externalElapsedMs)
    ? <DesktopPetIdleRigCanvas {...held} fallback={playback} /> : playback;
}

function LegacyDesktopPetCanvas({
  src,
  groomingSrc,
  motionClips,
  motionName,
  repeatMotion = false,
  state,
  lookAngle = null,
  className,
  label = "Animated desktop pet",
  reducedMotion = false,
  clip,
}: DesktopPetCanvasProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const images = useRef<Record<string, HTMLImageElement>>({});
  const motionRef = useRef(motionClips);
  motionRef.current = motionClips;
  const motionSignature = JSON.stringify(motionClips ?? {});
  const playback = useRef({
    state,
    lookAngle,
    reducedMotion,
    clip,
    motionName,
    repeatMotion,
  });
  const frameRequest = useRef(0);
  const startedAt = useRef<number | null>(null);
  const lastTick = useRef<number | null>(null);
  const gaze = useRef(0);
  const gazeFrame = useRef<number | null>(null);
  const lastPaint = useRef("");
  const transition = useRef<{
    image: HTMLCanvasElement;
    start: number | null;
  } | null>(null);
  const [failed, setFailed] = useState<string[]>([]);

  const captureTransition = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas || !lastPaint.current || playback.current.reducedMotion) {
      transition.current = null;
      return;
    }
    const snapshot = document.createElement("canvas");
    snapshot.width = canvas.width;
    snapshot.height = canvas.height;
    snapshot.getContext("2d")?.drawImage(canvas, 0, 0);
    transition.current = { image: snapshot, start: null };
  }, []);

  const draw = useCallback(
    (timestamp: number) => {
      frameRequest.current = 0;
      const canvas = canvasRef.current;
      const current = playback.current;
      const motion = current.motionName
        ? motionRef.current?.[current.motionName]
        : undefined;
      let image = motion
        ? images.current["motion:" + current.motionName]
        : current.clip === "grooming"
          ? images.current.grooming
          : images.current.main;
      // Hold the current pose until the requested, validated atlas has decoded.
      if (!canvas || !image) return;
      const context = canvas.getContext("2d", { willReadFrequently: true });
      if (!context) return;
      if (startedAt.current == null) startedAt.current = timestamp;
      const dt = lastTick.current == null ? 16 : timestamp - lastTick.current;
      lastTick.current = timestamp;
      let frame;
      let frameWidth: number = DESKTOP_PET_CELL.width;
      let frameHeight: number = DESKTOP_PET_CELL.height;
      let motionDone = false;
      let gazeMoving = false;
      if (motion) {
        const sampled = motionFrame(
          motion,
          timestamp - startedAt.current,
          current.reducedMotion,
          current.repeatMotion,
        );
        frame = sampled;
        frameWidth = motion.frameWidth;
        frameHeight = motion.frameHeight;
        motionDone = sampled.done;
        if (motionUsesNeutralFrame(motion, sampled.row, sampled.column)) {
          image = images.current.main;
          frame = { row: 0, column: 0 };
          frameWidth = DESKTOP_PET_CELL.width;
          frameHeight = DESKTOP_PET_CELL.height;
        }
      } else if (current.clip === "grooming") {
        frame = groomingFrame(
          timestamp - startedAt.current,
          current.reducedMotion,
        );
      } else if (
        !current.reducedMotion &&
        current.state === "look" &&
        current.lookAngle != null
      ) {
        gaze.current = advancePetGaze(gaze.current, current.lookAngle, dt);
        gazeMoving =
          Math.abs(shortestAngleDelta(gaze.current, current.lookAngle)) > 0.3;
        const index = stablePetGazeFrame(gaze.current, gazeFrame.current);
        gazeFrame.current = index;
        frame = { row: index < 8 ? 9 : 10, column: index % 8 };
      } else {
        frame = frameForElapsed(
          current.state === "look" ? "idle" : current.state,
          timestamp - startedAt.current,
          current.reducedMotion,
        );
      }
      if (!image) return;
      const ratio = Math.min(window.devicePixelRatio || 1, 2);
      const width = Math.round(DESKTOP_PET_CELL.width * ratio);
      const height = Math.round(DESKTOP_PET_CELL.height * ratio);
      const resized = canvas.width !== width || canvas.height !== height;
      const key = [
        motion ? "motion:" + current.motionName : (current.clip ?? "main"),
        frame.row,
        frame.column,
      ].join(":");
      if (
        !motion &&
        current.state === "look" &&
        lastPaint.current &&
        key !== lastPaint.current
      )
        captureTransition();
      if (resized) {
        canvas.width = width;
        canvas.height = height;
      }
      if (key !== lastPaint.current || resized || transition.current) {
        const blend = transition.current;
        if (blend && blend.start == null) blend.start = timestamp;
        const amount =
          current.reducedMotion || !blend
            ? 1
            : Math.min(1, (timestamp - (blend.start ?? timestamp)) / 90);
        context.setTransform(ratio, 0, 0, ratio, 0, 0);
        context.clearRect(
          0,
          0,
          DESKTOP_PET_CELL.width,
          DESKTOP_PET_CELL.height,
        );
        context.imageSmoothingEnabled = true;
        context.imageSmoothingQuality = "high";
        if (blend && amount < 1) {
          context.globalAlpha = 1 - amount;
          context.drawImage(
            blend.image,
            0,
            0,
            DESKTOP_PET_CELL.width,
            DESKTOP_PET_CELL.height,
          );
        }
        context.globalAlpha = amount;
        // Add premultiplied contributions; source-over would dim overlapping fur.
        context.globalCompositeOperation = "lighter";
        context.drawImage(
          image,
          frame.column * frameWidth,
          frame.row * frameHeight,
          frameWidth,
          frameHeight,
          (192 - frameWidth * Math.min(192 / frameWidth, 208 / frameHeight)) /
            2,
          208 - frameHeight * Math.min(192 / frameWidth, 208 / frameHeight),
          frameWidth * Math.min(192 / frameWidth, 208 / frameHeight),
          frameHeight * Math.min(192 / frameWidth, 208 / frameHeight),
        );
        context.globalAlpha = 1;
        context.globalCompositeOperation = "source-over";
        lastPaint.current = key;
        if (amount >= 1) transition.current = null;
      }
      if (
        !current.reducedMotion &&
        (!motionDone || transition.current) &&
        (current.state !== "look" || gazeMoving || transition.current)
      ) {
        frameRequest.current = window.requestAnimationFrame(draw);
      }
    },
    [captureTransition],
  );

  const requestDraw = useCallback(() => {
    if (!frameRequest.current)
      frameRequest.current = window.requestAnimationFrame(draw);
  }, [draw]);

  useEffect(() => {
    images.current = {};
    setFailed([]);
    startedAt.current = null;
    lastTick.current = null;
    lastPaint.current = "";
    transition.current = null;
    if (frameRequest.current) window.cancelAnimationFrame(frameRequest.current);
    frameRequest.current = 0;
    const canvas = canvasRef.current;
    if (canvas) {
      const context = canvas.getContext("2d", { willReadFrequently: true });
      context?.setTransform(1, 0, 0, 1, 0, 0);
      context?.clearRect(0, 0, canvas.width, canvas.height);
    }
    const stops = [
      {
        key: "main",
        path: src,
        kind: undefined,
        dimensions: undefined as { width: number; height: number } | undefined,
      },
      {
        key: "grooming" as const,
        path: groomingSrc,
        kind: "grooming" as const,
        dimensions: undefined,
      },
      ...Object.entries(JSON.parse(motionSignature) as PetMotionClips).map(
        ([name, spec]) => ({
          key: "motion:" + name,
          path: resolveMediaSrc(spec.path) || undefined,
          kind: undefined,
          dimensions: {
            width: spec.columns * spec.frameWidth,
            height:
              Math.ceil(spec.durationsMs.length / spec.columns) *
              spec.frameHeight,
          },
        }),
      ),
    ]
      .filter(({ path }) => Boolean(path))
      .map(({ key, path, kind, dimensions }) =>
        loadPetAtlas({
          src: path!,
          kind,
          dimensions,
          createImage: () => new Image(),
          loaded: (image) => {
            images.current[key] = image;
            requestDraw();
          },
          failed: () => setFailed((previous) => [...previous, key]),
        }),
      );
    return () => {
      stops.forEach((stop) => stop());
      images.current = {};
      if (frameRequest.current)
        window.cancelAnimationFrame(frameRequest.current);
      frameRequest.current = 0;
    };
  }, [requestDraw, src, groomingSrc, motionSignature]);

  useLayoutEffect(() => {
    const previous = playback.current;
    playback.current = {
      state,
      lookAngle,
      reducedMotion,
      clip,
      motionName,
      repeatMotion,
    };
    if (
      previous.state !== state ||
      previous.clip !== clip ||
      previous.motionName !== motionName ||
      previous.reducedMotion !== reducedMotion
    ) {
      if (previous.motionName || motionName) transition.current = null;
      else captureTransition();
      startedAt.current = null;
      lastTick.current = null;
      if (state === "look" && previous.state !== "look") {
        gaze.current = lookAngle ?? 0;
        gazeFrame.current = null;
      }
      lastPaint.current = "";
    }
    requestDraw();
  }, [
    state,
    lookAngle,
    reducedMotion,
    clip,
    motionName,
    repeatMotion,
    captureTransition,
    requestDraw,
  ]);

  return (
    <>
      <canvas
        ref={canvasRef}
        className={className}
        role="img"
        aria-label={label}
      />
      {failed.includes(
        motionName ? "motion:" + motionName : (clip ?? "main"),
      ) && (
        <span className="desktop-pet-image-error" role="alert">
          {label}: 图片无法加载 / Image unavailable
        </span>
      )}
    </>
  );
}
