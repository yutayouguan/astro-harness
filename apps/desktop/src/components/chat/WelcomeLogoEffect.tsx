import { useEffect, useRef, type CSSProperties } from "react";
import { ASTRO_MARK_PATH, AstroLogoMark } from "../icons/AstroLogoMark";

const LOGO_MASK = `url("data:image/svg+xml,${encodeURIComponent(
  `<svg xmlns="http://www.w3.org/2000/svg" viewBox="205 127 436 408"><path fill="white" d="${ASTRO_MARK_PATH}"/></svg>`,
)}")`;

const LED_SHADER = /* wgsl */ `
struct Viewport {
  resolution: vec2f,
  dark_mode: f32,
  padding: f32,
  tone: vec4f,
  accent: vec4f,
}

struct Motion {
  pointer: vec2f,
  time: f32,
  pointer_active: f32,
}

@group(0) @binding(0) var<uniform> viewport: Viewport;
@group(0) @binding(1) var<uniform> motion: Motion;

@fragment fn fs_main(@location(0) uv: vec2f) -> @location(0) vec4f {
  let p = uv * 2.0 - 1.0;
  let pointer = motion.pointer * 2.0 - 1.0;
  let normal = normalize(vec3f(p.x * 0.46, -p.y * 0.38, 1.0));
  let light_direction = normalize(vec3f(pointer.x - p.x, p.y - pointer.y, 0.72));
  let view_direction = vec3f(0.0, 0.0, 1.0);
  let half_direction = normalize(light_direction + view_direction);
  let diffuse = 0.42 + max(dot(normal, light_direction), 0.0) * 0.64;
  let specular = pow(max(dot(normal, half_direction), 0.0), 34.0);
  let rim = pow(1.0 - max(normal.z, 0.0), 2.2);

  let tone = viewport.tone.rgb;
  let accent = viewport.accent.rgb;
  let base = mix(
    mix(tone * 0.48, tone, smoothstep(0.08, 0.88, uv.x)),
    accent,
    smoothstep(0.58, 1.0, uv.y) * 0.34,
  );
  let sweep_position = fract(motion.time * 0.075) * 2.5 - 0.35;
  let sweep = exp(-abs(uv.x + uv.y * 0.42 - sweep_position) * 28.0);
  let pointer_glow = motion.pointer_active * exp(-distance(uv, motion.pointer) * 5.4);
  let theme_lift = mix(0.92, 1.08, viewport.dark_mode);
  let colour = base * diffuse * theme_lift
    + vec3f(1.0, 0.98, 0.96) * specular * 1.18
    + accent * (sweep * 0.38 + rim * 0.34 + pointer_glow * 0.28);

  return vec4f(colour, 0.96);
}
`;

type PointerState = {
  x: number;
  y: number;
  active: number;
};

type RgbaVector = [number, number, number, number];

function resolveThemeColor(
  root: HTMLElement,
  variable: "--tone" | "--accent-2",
  fallback: string,
): RgbaVector {
  const probe = document.createElement("span");
  probe.style.cssText = `position:absolute;visibility:hidden;color:var(${variable}, ${fallback})`;
  root.append(probe);
  const channels = getComputedStyle(probe)
    .color.match(/[\d.]+/g)
    ?.map(Number);
  probe.remove();

  if (!channels || channels.length < 3) {
    return variable === "--tone"
      ? [0.145, 0.388, 0.922, 1]
      : [0.231, 0.51, 0.965, 1];
  }

  return [channels[0] / 255, channels[1] / 255, channels[2] / 255, 1];
}

function readThemeColors(root: HTMLElement) {
  return {
    tone: resolveThemeColor(root, "--tone", "#2563eb"),
    accent: resolveThemeColor(root, "--accent-2", "#3b82f6"),
  };
}

function isDarkTheme(): boolean {
  return document.documentElement.dataset.theme === "dark";
}

export function WelcomeLogoEffect() {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !("gpu" in navigator)) return;

    let cancelled = false;
    let disposeRenderer: (() => void) | undefined;

    const initialize = async () => {
      const { clock, effect, frame, frameLoop, init, surface } = await import(
        "vgpu"
      );
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

      const pointer: PointerState = { x: 0.38, y: 0.28, active: 0 };
      const themeRoot = canvas.parentElement ?? document.documentElement;
      const themeColors = readThemeColors(themeRoot);
      const ledEffect = effect(gpu, LED_SHADER, {
        set: {
          viewport: {
            resolution: [canvasSurface.size[0], canvasSurface.size[1]],
            dark_mode: isDarkTheme() ? 1 : 0,
            padding: 0,
            tone: themeColors.tone,
            accent: themeColors.accent,
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

      const interactionRoot = canvas.closest<HTMLElement>(".chat-welcome-mark");
      const updatePointer = (event: PointerEvent) => {
        const bounds = canvas.getBoundingClientRect();
        pointer.x = Math.min(
          1,
          Math.max(
            0,
            (event.clientX - bounds.left) / Math.max(1, bounds.width),
          ),
        );
        pointer.y = Math.min(
          1,
          Math.max(
            0,
            (event.clientY - bounds.top) / Math.max(1, bounds.height),
          ),
        );
        pointer.active =
          event.clientX >= bounds.left &&
          event.clientX <= bounds.right &&
          event.clientY >= bounds.top &&
          event.clientY <= bounds.bottom
            ? 1
            : 0;
      };
      const clearPointer = () => {
        pointer.active = 0;
      };
      interactionRoot?.addEventListener("pointermove", updatePointer, {
        passive: true,
      });
      interactionRoot?.addEventListener("pointerleave", clearPointer);

      let dark = isDarkTheme();
      const themeObserver = new MutationObserver(() => {
        dark = isDarkTheme();
        const colors = readThemeColors(themeRoot);
        ledEffect.set({
          viewport: {
            dark_mode: dark ? 1 : 0,
            tone: colors.tone,
            accent: colors.accent,
          },
        });
      });
      themeObserver.observe(document.documentElement, {
        attributes: true,
        attributeFilter: [
          "data-theme",
          "data-tone",
          "data-color-style",
          "style",
        ],
      });

      const reducedMotion = window.matchMedia(
        "(prefers-reduced-motion: reduce)",
      ).matches;
      const timer = clock(gpu);
      const draw = (
        currentFrame: Parameters<Parameters<typeof frameLoop>[1]>[0],
        time: number,
      ) => {
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
        loop = frameLoop(gpu, (currentFrame) =>
          draw(currentFrame, timer.time % 4096),
        );
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

  const maskStyle = { "--welcome-logo-mask": LOGO_MASK } as CSSProperties;

  return (
    <span className="chat-welcome-logo-stack" style={maskStyle}>
      <AstroLogoMark
        className="chat-welcome-logo chat-welcome-logo--depth chat-welcome-logo--depth-far"
        width={72}
        height={72}
      />
      <AstroLogoMark
        className="chat-welcome-logo chat-welcome-logo--depth chat-welcome-logo--depth-near"
        width={72}
        height={72}
      />
      <AstroLogoMark
        className="chat-welcome-logo chat-welcome-logo--front"
        width={72}
        height={72}
      />
      <canvas
        ref={canvasRef}
        className="chat-welcome-logo-lighting"
        aria-hidden
      />
      <span className="chat-welcome-logo-lighting-fallback" aria-hidden />
    </span>
  );
}
