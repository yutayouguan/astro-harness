import { useEffect, useRef } from "react";

const PARTICLE_COUNT = 26;
const SPEED = 0.12;

const PALETTE_DARK = [
  [139, 92, 246], // purple
  [99, 102, 241], // indigo
  [59, 130, 246], // blue
  [14, 165, 233], // sky
  [236, 72, 153], // pink
  [168, 85, 247], // violet
  [34, 211, 238], // cyan
  [251, 191, 36], // amber
];

const PALETTE_LIGHT = [
  [124, 58, 237], // purple
  [79, 70, 229], // indigo
  [37, 99, 235], // blue
  [2, 132, 199], // sky
  [219, 39, 119], // pink
  [139, 92, 246], // violet
  [8, 145, 178], // cyan
  [217, 119, 6], // amber
];

type Particle = {
  x: number;
  y: number;
  r: number;
  dx: number;
  dy: number;
  alpha: number;
  phase: number;
  colorIdx: number;
  glow: boolean;
};

function createParticles(w: number, h: number): Particle[] {
  return Array.from({ length: PARTICLE_COUNT }, () => ({
    x: Math.random() * w,
    y: Math.random() * h,
    r: 1 + Math.random() * 2,
    dx: (Math.random() - 0.5) * SPEED,
    dy: (Math.random() - 0.5) * SPEED - 0.04,
    alpha: 0.2 + Math.random() * 0.34,
    phase: Math.random() * Math.PI * 2,
    colorIdx: Math.floor(Math.random() * PALETTE_DARK.length),
    glow: Math.random() > 0.78,
  }));
}

export default function ParticleField() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const rafRef = useRef(0);
  const particlesRef = useRef<Particle[]>([]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const mq = window.matchMedia("(prefers-reduced-motion: reduce)");
    if (mq.matches) return;

    const ctx = canvas.getContext("2d", { alpha: true });
    if (!ctx) return;

    const resize = () => {
      const rect = canvas.parentElement?.getBoundingClientRect();
      if (!rect) return;
      const dpr = window.devicePixelRatio || 1;
      canvas.width = rect.width * dpr;
      canvas.height = rect.height * dpr;
      canvas.style.width = `${rect.width}px`;
      canvas.style.height = `${rect.height}px`;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      particlesRef.current = createParticles(rect.width, rect.height);
    };

    resize();
    const ro = new ResizeObserver(resize);
    ro.observe(canvas.parentElement!);

    let t = 0;
    const dark = () =>
      document.documentElement.getAttribute("data-theme") === "dark";

    const draw = () => {
      const w = canvas.width / (window.devicePixelRatio || 1);
      const h = canvas.height / (window.devicePixelRatio || 1);
      ctx.clearRect(0, 0, w, h);
      t += 0.006;

      const isDark = dark();
      const palette = isDark ? PALETTE_DARK : PALETTE_LIGHT;

      for (const p of particlesRef.current) {
        p.x += p.dx;
        p.y += p.dy;

        if (p.x < -10) p.x = w + 10;
        if (p.x > w + 10) p.x = -10;
        if (p.y < -10) p.y = h + 10;
        if (p.y > h + 10) p.y = -10;

        const twinkle = 0.45 + 0.55 * Math.sin(t * 2 + p.phase);
        const a = p.alpha * twinkle;
        const [cr, cg, cb] = palette[p.colorIdx];

        if (p.glow) {
          const grad = ctx.createRadialGradient(p.x, p.y, 0, p.x, p.y, p.r * 4);
          grad.addColorStop(
            0,
            `rgba(${cr},${cg},${cb},${(a * 0.6).toFixed(3)})`,
          );
          grad.addColorStop(
            0.4,
            `rgba(${cr},${cg},${cb},${(a * 0.2).toFixed(3)})`,
          );
          grad.addColorStop(1, `rgba(${cr},${cg},${cb},0)`);
          ctx.beginPath();
          ctx.arc(p.x, p.y, p.r * 4, 0, Math.PI * 2);
          ctx.fillStyle = grad;
          ctx.fill();
        }

        ctx.beginPath();
        ctx.arc(p.x, p.y, p.r, 0, Math.PI * 2);
        ctx.fillStyle = `rgba(${cr},${cg},${cb},${a.toFixed(3)})`;
        ctx.fill();
      }

      rafRef.current = requestAnimationFrame(draw);
    };

    rafRef.current = requestAnimationFrame(draw);

    return () => {
      cancelAnimationFrame(rafRef.current);
      ro.disconnect();
    };
  }, []);

  return (
    <canvas ref={canvasRef} className="chat-welcome-particles" aria-hidden />
  );
}
