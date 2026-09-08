import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { loadPetAtlas } from "../../lib/ui/desktopPetAtlas";

import {
  DESKTOP_PET_CELL,
  frameForElapsed,
  frameForLookAngle,
  type DesktopPetAnimationState,
} from "../../lib/ui/desktopPetAnimation";

type Props = {
  src: string;
  state: DesktopPetAnimationState;
  lookAngle?: number | null;
  className?: string;
  label?: string;
  reducedMotion?: boolean;
};

export default function DesktopPetCanvas({
  src,
  state,
  lookAngle = null,
  className,
  label = "Animated desktop pet",
  reducedMotion = false,
}: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const imageRef = useRef<HTMLImageElement | null>(null);
  const frameRequestRef = useRef(0);
  const startedAtRef = useRef(0);
  const playbackRef = useRef({ state, lookAngle, reducedMotion });
  const [loadError, setLoadError] = useState(false);

  const draw = useCallback((timestamp: number) => {
    const canvas = canvasRef.current;
    const image = imageRef.current;
    if (!canvas || !image) return;
    const context = canvas.getContext("2d");
    if (!context) return;
    if (!startedAtRef.current) startedAtRef.current = timestamp;
    const playback = playbackRef.current;
    const frame =
      !playback.reducedMotion &&
      playback.state === "look" &&
      playback.lookAngle != null
        ? frameForLookAngle(playback.lookAngle)
        : frameForElapsed(
            playback.state === "look" ? "idle" : playback.state,
            timestamp - startedAtRef.current,
            playback.reducedMotion,
          );
    const ratio = Math.min(window.devicePixelRatio || 1, 2);
    const targetWidth = Math.round(DESKTOP_PET_CELL.width * ratio);
    const targetHeight = Math.round(DESKTOP_PET_CELL.height * ratio);
    if (canvas.width !== targetWidth || canvas.height !== targetHeight) {
      canvas.width = targetWidth;
      canvas.height = targetHeight;
    }
    context.setTransform(ratio, 0, 0, ratio, 0, 0);
    context.clearRect(0, 0, DESKTOP_PET_CELL.width, DESKTOP_PET_CELL.height);
    context.imageSmoothingEnabled = true;
    context.imageSmoothingQuality = "high";
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
    if (!playback.reducedMotion && playback.state !== "look") {
      frameRequestRef.current = window.requestAnimationFrame(draw);
    }
  }, []);

  const restart = useCallback(() => {
    if (frameRequestRef.current) {
      window.cancelAnimationFrame(frameRequestRef.current);
    }
    startedAtRef.current = 0;
    if (imageRef.current) {
      frameRequestRef.current = window.requestAnimationFrame(draw);
    }
  }, [draw]);

  useEffect(() => {
    imageRef.current = null;
    setLoadError(false);
    if (frameRequestRef.current)
      window.cancelAnimationFrame(frameRequestRef.current);
    const canvas = canvasRef.current;
    const context = canvas?.getContext("2d");
    if (canvas && context) {
      context.setTransform(1, 0, 0, 1, 0, 0);
      context.clearRect(0, 0, canvas.width, canvas.height);
    }
    if (!src) return;
    const dispose = loadPetAtlas({
      src,
      createImage: () => new Image(),
      loaded: (image) => {
        imageRef.current = image;
        restart();
      },
      failed: () => setLoadError(true),
    });
    return () => {
      dispose();
      imageRef.current = null;
      if (frameRequestRef.current) {
        window.cancelAnimationFrame(frameRequestRef.current);
      }
    };
  }, [restart, src]);

  useLayoutEffect(() => {
    playbackRef.current = { state, lookAngle, reducedMotion };
    restart();
  }, [lookAngle, reducedMotion, restart, state]);

  return (
    <>
      <canvas
        ref={canvasRef}
        className={className}
        role="img"
        aria-label={label}
      />
      {loadError ? (
        <span className="desktop-pet-image-error" role="alert">
          {label}: 图片无法加载 / Image unavailable
        </span>
      ) : null}
    </>
  );
}
