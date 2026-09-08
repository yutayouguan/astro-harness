import { useCallback, useEffect, useRef } from "react";

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
  playbackRef.current = { state, lookAngle, reducedMotion };

  const draw = useCallback((timestamp: number) => {
    const canvas = canvasRef.current;
    const image = imageRef.current;
    if (!canvas || !image) return;
    const context = canvas.getContext("2d");
    if (!context) return;
    if (!startedAtRef.current) startedAtRef.current = timestamp;
    const playback = playbackRef.current;
    const frame =
      playback.state === "look" && playback.lookAngle != null
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
    if (!src) return;
    const image = new Image();
    let disposed = false;

    image.onload = () => {
      if (disposed) return;
      imageRef.current = image;
      restart();
    };
    image.src = src;
    return () => {
      disposed = true;
      image.onload = null;
      imageRef.current = null;
      if (frameRequestRef.current) {
        window.cancelAnimationFrame(frameRequestRef.current);
      }
    };
  }, [restart, src]);

  useEffect(() => {
    restart();
  }, [lookAngle, reducedMotion, restart, state]);

  return (
    <canvas
      ref={canvasRef}
      className={className}
      role="img"
      aria-label={label}
    />
  );
}
