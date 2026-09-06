import { useEffect, useRef } from "react";
import { shouldStartCompletionCelebration } from "../../lib/chat/taskCompletion";

type Props = {
  /** 单调递增；每次变化都重新播放一次庆祝动画。 */
  trigger: number;
};

type ConfettiPiece = {
  x: number;
  y: number;
  vx: number;
  vy: number;
  gravity: number;
  drag: number;
  width: number;
  height: number;
  rotation: number;
  spin: number;
  color: string;
  bornAt: number;
  life: number;
  shape: "rect" | "circle";
};

const COLORS = [
  "#2563eb",
  "#7c3aed",
  "#ec4899",
  "#f59e0b",
  "#10b981",
  "#06b6d4",
  "#ef4444",
];

const randomBetween = (min: number, max: number) =>
  min + Math.random() * (max - min);

function drawPiece(
  context: CanvasRenderingContext2D,
  piece: ConfettiPiece,
  alpha: number,
) {
  context.save();
  context.globalAlpha = alpha;
  context.fillStyle = piece.color;
  context.translate(piece.x, piece.y);
  context.rotate(piece.rotation);

  if (piece.shape === "circle") {
    context.beginPath();
    context.arc(0, 0, piece.width / 2, 0, Math.PI * 2);
    context.fill();
  } else {
    const flutter = Math.max(0.18, Math.abs(Math.cos(piece.rotation)));
    context.scale(1, flutter);
    context.fillRect(
      -piece.width / 2,
      -piece.height / 2,
      piece.width,
      piece.height,
    );
  }

  context.restore();
}

function createPieces(width: number, height: number, startedAt: number) {
  const density = Math.min(176, Math.max(112, Math.round(width / 8)));
  const rainCount = Math.round(density * 0.58);
  const pieces: ConfettiPiece[] = [];

  for (let index = 0; index < rainCount; index += 1) {
    pieces.push({
      x: randomBetween(0, width),
      y: randomBetween(-height * 0.42, -12),
      vx: randomBetween(-45, 45),
      vy: randomBetween(190, 360),
      gravity: randomBetween(28, 58),
      drag: randomBetween(0.985, 0.996),
      width: randomBetween(5, 10),
      height: randomBetween(8, 16),
      rotation: randomBetween(0, Math.PI * 2),
      spin: randomBetween(-7, 7),
      color: COLORS[index % COLORS.length]!,
      bornAt: startedAt + randomBetween(0, 260),
      life: randomBetween(2_000, 2_700),
      shape: index % 6 === 0 ? "circle" : "rect",
    });
  }

  for (let index = rainCount; index < density; index += 1) {
    const fromLeft = index % 2 === 0;
    pieces.push({
      x: fromLeft
        ? randomBetween(-8, width * 0.08)
        : randomBetween(width * 0.92, width + 8),
      y: randomBetween(height * 0.68, height * 0.94),
      vx: (fromLeft ? 1 : -1) * randomBetween(width * 0.26, width * 0.58),
      vy: randomBetween(-height * 0.92, -height * 0.58),
      gravity: randomBetween(520, 710),
      drag: randomBetween(0.982, 0.994),
      width: randomBetween(6, 11),
      height: randomBetween(9, 17),
      rotation: randomBetween(0, Math.PI * 2),
      spin: randomBetween(-10, 10),
      color: COLORS[index % COLORS.length]!,
      bornAt: startedAt + randomBetween(0, 180),
      life: randomBetween(1_750, 2_350),
      shape: index % 7 === 0 ? "circle" : "rect",
    });
  }

  return pieces;
}

export default function TaskCompletionCelebration({ trigger }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const previousTriggerRef = useRef(trigger);

  useEffect(() => {
    const previousTrigger = previousTriggerRef.current;
    previousTriggerRef.current = trigger;
    if (!shouldStartCompletionCelebration(previousTrigger, trigger)) return;
    const canvas = canvasRef.current;
    const context = canvas?.getContext("2d");
    if (!canvas || !context) return;

    const bounds = canvas.getBoundingClientRect();
    if (bounds.width <= 0 || bounds.height <= 0) return;

    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    canvas.width = Math.round(bounds.width * dpr);
    canvas.height = Math.round(bounds.height * dpr);
    context.setTransform(dpr, 0, 0, dpr, 0, 0);
    context.clearRect(0, 0, bounds.width, bounds.height);

    const reducedMotion = window.matchMedia?.(
      "(prefers-reduced-motion: reduce)",
    ).matches;
    let animationFrame = 0;
    let clearTimer = 0;
    let released = false;
    const releaseCanvasBackingStore = () => {
      if (released) return;
      released = true;
      canvas.width = 1;
      canvas.height = 1;
    };

    if (reducedMotion) {
      const startedAt = performance.now();
      const pieces = createPieces(bounds.width, bounds.height, startedAt).slice(
        0,
        28,
      );
      for (const [index, piece] of pieces.entries()) {
        piece.x = ((index + 0.5) / pieces.length) * bounds.width;
        piece.y = bounds.height * randomBetween(0.18, 0.62);
        drawPiece(context, piece, 0.78);
      }
      clearTimer = window.setTimeout(releaseCanvasBackingStore, 560);
    } else {
      const startedAt = performance.now();
      const pieces = createPieces(bounds.width, bounds.height, startedAt);
      let previousAt = startedAt;

      const animate = (now: number) => {
        const deltaSeconds = Math.min((now - previousAt) / 1_000, 0.034);
        previousAt = now;
        context.clearRect(0, 0, bounds.width, bounds.height);

        let active = false;
        for (const piece of pieces) {
          const age = now - piece.bornAt;
          if (age < 0 || age > piece.life) continue;
          active = true;
          piece.vx *= piece.drag;
          piece.vy += piece.gravity * deltaSeconds;
          piece.x += piece.vx * deltaSeconds;
          piece.y += piece.vy * deltaSeconds;
          piece.rotation += piece.spin * deltaSeconds;
          const fade = Math.min(1, age / 120, (piece.life - age) / 420);
          drawPiece(context, piece, Math.max(0, fade) * 0.9);
        }

        if (active || now - startedAt < 320) {
          animationFrame = window.requestAnimationFrame(animate);
        } else {
          releaseCanvasBackingStore();
        }
      };

      animationFrame = window.requestAnimationFrame(animate);
    }

    return () => {
      if (animationFrame) window.cancelAnimationFrame(animationFrame);
      if (clearTimer) window.clearTimeout(clearTimer);
      releaseCanvasBackingStore();
    };
  }, [trigger]);

  return (
    <canvas
      ref={canvasRef}
      className="task-completion-celebration"
      data-testid="task-completion-celebration"
      aria-hidden="true"
    />
  );
}
