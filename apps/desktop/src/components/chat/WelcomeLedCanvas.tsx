import { useEffect, useRef } from "react";

const LED_SHADER = /* wgsl */ `
struct Viewport {
  resolution: vec2f,
  dark_mode: f32,
  padding: f32,
}

struct Motion {
  pointer: vec2f,
  time: f32,
  pointer_active: f32,
}

@group(0) @binding(0) var<uniform> viewport: Viewport;
@group(0) @binding(1) var<uniform> motion: Motion;

fn segment_info(p: vec2f, a: vec2f, b: vec2f) -> vec2f {
  let ab = b - a;
  let t = clamp(dot(p - a, ab) / dot(ab, ab), 0.0, 1.0);
  return vec2f(length(p - (a + ab * t)), t);
}

fn side(a: vec2f, b: vec2f, p: vec2f) -> f32 {
  let edge = b - a;
  return edge.x * (p.y - a.y) - edge.y * (p.x - a.x);
}

fn inside_triangle(p: vec2f, a: vec2f, b: vec2f, c: vec2f) -> bool {
  let s0 = side(a, b, p);
  let s1 = side(b, c, p);
  let s2 = side(c, a, p);
  return (s0 <= 0.0 && s1 <= 0.0 && s2 <= 0.0) ||
    (s0 >= 0.0 && s1 >= 0.0 && s2 >= 0.0);
}

fn led_pattern(t: f32, phase: f32) -> f32 {
  let cell = abs(fract(t * 31.0 + phase) - 0.5);
  return 1.0 - smoothstep(0.23, 0.44, cell);
}

@fragment fn fs_main(@location(0) uv: vec2f) -> @location(0) vec4f {
  let resolution = max(viewport.resolution, vec2f(1.0));
  let aspect = resolution.x / resolution.y;
  let center = vec2f(0.5, 0.43);
  let p = (uv - center) * vec2f(aspect, 1.0);
  let pointer = (motion.pointer - center) * vec2f(aspect, 1.0);

  let radius = 0.255;
  let top = vec2f(0.0, -radius);
  let left = vec2f(-radius * 0.87, radius * 0.5);
  let right = vec2f(radius * 0.87, radius * 0.5);
  let edge0 = segment_info(p, top, left);
  let edge1 = segment_info(p, left, right);
  let edge2 = segment_info(p, right, top);
  let edge = min(edge0.x, min(edge1.x, edge2.x));

  let w0 = exp(-edge0.x * 95.0);
  let w1 = exp(-edge1.x * 95.0);
  let w2 = exp(-edge2.x * 95.0);
  let weight_sum = max(w0 + w1 + w2, 0.0001);
  let edge_colour = (
    vec3f(0.34, 0.60, 1.0) * w0 +
    vec3f(0.58, 0.31, 1.0) * w1 +
    vec3f(0.12, 0.82, 0.94) * w2
  ) / weight_sum;

  let time = motion.time;
  let led = max(
    led_pattern(edge0.y, time * 0.035) * w0,
    max(
      led_pattern(edge1.y, time * -0.028 + 0.27) * w1,
      led_pattern(edge2.y, time * 0.032 + 0.61) * w2,
    ),
  );
  let pointer_reveal = motion.pointer_active * exp(-distance(p, pointer) * 5.5);
  let travel = 0.5 + 0.5 * sin(time * 1.35 - edge * 34.0);
  let colour_mix = clamp(pointer_reveal * 1.45 + travel * 0.22, 0.0, 1.0);
  let light_colour = mix(vec3f(0.96, 0.98, 1.0), edge_colour, colour_mix);

  let crisp = exp(-edge * 120.0) * (0.46 + led * 0.92);
  let near_glow = exp(-edge * 31.0) * (0.18 + led * 0.24);
  let far_glow = exp(-edge * 8.0) * (0.035 + pointer_reveal * 0.11);
  let floor_distance = length(vec2f(p.x * 0.82, max(p.y - radius * 0.5, 0.0)));
  let floor_radiance = exp(-floor_distance * 5.2) * smoothstep(-0.04, 0.08, p.y - radius * 0.32) * 0.055;
  let pulse = 0.94 + 0.06 * sin(time * 1.8);
  let intensity = (crisp + near_glow + far_glow + floor_radiance) * pulse;

  let inside = inside_triangle(p, top, left, right);
  let dark_mode = viewport.dark_mode;
  let body_alpha = select(0.0, mix(0.12, 0.62, dark_mode), inside);
  let glow_alpha = clamp(intensity * 0.78, 0.0, 0.82);
  let alpha = max(body_alpha, glow_alpha);
  var colour = light_colour * glow_alpha;
  if (inside) {
    colour *= mix(0.28, 0.04, dark_mode);
  }

  return vec4f(colour, alpha);
}
`;

type PointerState = {
  x: number;
  y: number;
  active: number;
};

function isDarkTheme(): boolean {
  return document.documentElement.dataset.theme === "dark";
}

export function WelcomeLedCanvas() {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !("gpu" in navigator)) return;

    let cancelled = false;
    let disposeRenderer: (() => void) | undefined;

    const initialize = async () => {
      const { clock, effect, frame, frameLoop, init, surface } = await import("vgpu");
      if (cancelled) return;

      const gpu = await init();
      if (cancelled) {
        gpu.dispose();
        return;
      }

      const canvasSurface = surface(gpu, canvas, {
        alphaMode: "premultiplied",
        dpr: [1, 2],
        label: "astro-welcome-led-surface",
      });
      canvasSurface.clearColor = [0, 0, 0, 0];

      const pointer: PointerState = { x: 0.5, y: 0.43, active: 0 };
      const ledEffect = effect(gpu, LED_SHADER, {
        set: {
          viewport: {
            resolution: [canvasSurface.size[0], canvasSurface.size[1]],
            dark_mode: isDarkTheme() ? 1 : 0,
            padding: 0,
          },
          motion: {
            pointer: [pointer.x, pointer.y],
            time: 0,
            pointer_active: pointer.active,
          },
        },
      });

      disposeRenderer = () => {
        canvasSurface.dispose();
        gpu.dispose();
      };

      const unsubscribeResize = canvasSurface.onResize(({ width, height }) => {
        ledEffect.set({ viewport: { resolution: [width, height] } });
      });
      await ledEffect.compile(canvasSurface);
      if (cancelled) {
        unsubscribeResize();
        canvasSurface.dispose();
        gpu.dispose();
        return;
      }

      const interactionRoot = canvas.closest<HTMLElement>(".chat-welcome");
      const updatePointer = (event: PointerEvent) => {
        const bounds = canvas.getBoundingClientRect();
        pointer.x = Math.min(1, Math.max(0, (event.clientX - bounds.left) / Math.max(1, bounds.width)));
        pointer.y = Math.min(1, Math.max(0, (event.clientY - bounds.top) / Math.max(1, bounds.height)));
        pointer.active = event.clientX >= bounds.left && event.clientX <= bounds.right &&
          event.clientY >= bounds.top && event.clientY <= bounds.bottom ? 1 : 0;
      };
      const clearPointer = () => {
        pointer.active = 0;
      };
      interactionRoot?.addEventListener("pointermove", updatePointer, { passive: true });
      interactionRoot?.addEventListener("pointerleave", clearPointer);

      let dark = isDarkTheme();
      const themeObserver = new MutationObserver(() => {
        dark = isDarkTheme();
        ledEffect.set({ viewport: { dark_mode: dark ? 1 : 0 } });
      });
      themeObserver.observe(document.documentElement, {
        attributes: true,
        attributeFilter: ["data-theme"],
      });

      const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
      const timer = clock(gpu);
      const draw = (currentFrame: Parameters<Parameters<typeof frameLoop>[1]>[0], time: number) => {
        ledEffect.set({
          motion: {
            pointer: [pointer.x, pointer.y],
            time,
            pointer_active: pointer.active,
          },
        });
        currentFrame.pass(canvasSurface, ledEffect);
      };

      let loop: { stop(): void } | undefined;
      if (reducedMotion) {
        frame(gpu, (currentFrame) => draw(currentFrame, 0));
      } else {
        loop = frameLoop(gpu, (currentFrame) => draw(currentFrame, timer.time % 4096));
      }
      canvas.dataset.ready = "true";

      disposeRenderer = () => {
        loop?.stop();
        themeObserver.disconnect();
        interactionRoot?.removeEventListener("pointermove", updatePointer);
        interactionRoot?.removeEventListener("pointerleave", clearPointer);
        unsubscribeResize();
        canvasSurface.dispose();
        gpu.dispose();
      };
    };

    void initialize().catch(() => {
      canvas.dataset.ready = "false";
      disposeRenderer?.();
      disposeRenderer = undefined;
    });

    return () => {
      cancelled = true;
      disposeRenderer?.();
    };
  }, []);

  return <canvas ref={canvasRef} className="chat-welcome-led-canvas" aria-hidden />;
}
