import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { loadPetAtlas } from "../../lib/ui/desktopPetAtlas";
import { groomingFrame } from "../../lib/ui/desktopPetLeisure";
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

type Props = {
  src: string;
  groomingSrc?: string;
  state: DesktopPetAnimationState;
  lookAngle?: number | null;
  className?: string;
  label?: string;
  reducedMotion?: boolean;
  clip?: "grooming";
};

export default function DesktopPetCanvas({
  src,
  groomingSrc,
  state,
  lookAngle = null,
  className,
  label = "Animated desktop pet",
  reducedMotion = false,
  clip,
}: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const images = useRef<{
    main?: HTMLImageElement;
    grooming?: HTMLImageElement;
  }>({});
  const playback = useRef({ state, lookAngle, reducedMotion, clip });
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
      const image =
        current.clip === "grooming"
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
      let gazeMoving = false;
      if (current.clip === "grooming") {
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
      const ratio = Math.min(window.devicePixelRatio || 1, 2);
      const width = Math.round(DESKTOP_PET_CELL.width * ratio);
      const height = Math.round(DESKTOP_PET_CELL.height * ratio);
      const resized = canvas.width !== width || canvas.height !== height;
      const key = [current.clip ?? "main", frame.row, frame.column].join(":");
      if (
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
          frame.column * DESKTOP_PET_CELL.width,
          frame.row * DESKTOP_PET_CELL.height,
          DESKTOP_PET_CELL.width,
          DESKTOP_PET_CELL.height,
          0,
          0,
          DESKTOP_PET_CELL.width,
          DESKTOP_PET_CELL.height,
        );
        context.globalAlpha = 1;
        context.globalCompositeOperation = "source-over";
        lastPaint.current = key;
        if (amount >= 1) transition.current = null;
      }
      if (
        !current.reducedMotion &&
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
      { key: "main" as const, path: src, kind: undefined },
      {
        key: "grooming" as const,
        path: groomingSrc,
        kind: "grooming" as const,
      },
    ]
      .filter(({ path }) => Boolean(path))
      .map(({ key, path, kind }) =>
        loadPetAtlas({
          src: path!,
          kind,
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
  }, [requestDraw, src, groomingSrc]);

  useLayoutEffect(() => {
    const previous = playback.current;
    playback.current = { state, lookAngle, reducedMotion, clip };
    if (
      previous.state !== state ||
      previous.clip !== clip ||
      previous.reducedMotion !== reducedMotion
    ) {
      captureTransition();
      startedAt.current = null;
      lastTick.current = null;
      if (state === "look" && previous.state !== "look") {
        gaze.current = lookAngle ?? 0;
        gazeFrame.current = null;
      }
      lastPaint.current = "";
    }
    requestDraw();
  }, [state, lookAngle, reducedMotion, clip, captureTransition, requestDraw]);

  return (
    <>
      <canvas
        ref={canvasRef}
        className={className}
        role="img"
        aria-label={label}
      />
      {failed.includes(clip ?? "main") && (
        <span className="desktop-pet-image-error" role="alert">
          {label}: 图片无法加载 / Image unavailable
        </span>
      )}
    </>
  );
}
