import { useEffect, useRef } from "react";

const PARTICLE_COUNT = 40;
const SPEED = 0.15;

type Particle = {
  x: number;
  y: number;
  r: number;
  dx: number;
  dy: number;
  alpha: number;
  phase: number;
};

function createParticles(w: number, h: number): Particle[] {
  return Array.from({ length: PARTICLE_COUNT }, () => ({
    x: Math.random() * w,
    y: Math.random() * h,
    r: 0.6 + Math.random() * 1.4,
    dx: (Math.random() - 0.5) * SPEED,
    dy: (Math.random() - 0.5) * SPEED - 0.06,
    alpha: 0.15 + Math.random() * 0.35,
    phase: Math.random() * Math.PI * 2,
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
      t += 0.008;

      const isDark = dark();
      const baseColor = isDark ? "255,255,255" : "100,116,139";

      for (const p of particlesRef.current) {
        p.x += p.dx;
        p.y += p.dy;

        if (p.x < -10) p.x = w + 10;
        if (p.x > w + 10) p.x = -10;
        if (p.y < -10) p.y = h + 10;
        if (p.y > h + 10) p.y = -10;

        const twinkle = 0.5 + 0.5 * Math.sin(t * 2.5 + p.phase);
        const a = p.alpha * twinkle;

        ctx.beginPath();
        ctx.arc(p.x, p.y, p.r, 0, Math.PI * 2);
        ctx.fillStyle = `rgba(${baseColor},${a.toFixed(3)})`;
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
    <canvas
      ref={canvasRef}
      className="chat-welcome-particles"
      aria-hidden
    />
  );
}
