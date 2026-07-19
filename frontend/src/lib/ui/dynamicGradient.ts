/** 灵动配色：本机种子 + tabId 确定性哈希出安全渐变。 */
import type { ShellGradient } from "./shellGradient";

/** FNV-1a 32-bit */
export function hashString(input: string): number {
  let h = 2166136261;
  for (let i = 0; i < input.length; i += 1) {
    h ^= input.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

function clamp01(n: number): number {
  return Math.min(1, Math.max(0, n));
}

/** h∈[0,360), s/l∈[0,100] → #RRGGBB */
export function hslToHex(h: number, s: number, l: number): string {
  const hh = ((h % 360) + 360) % 360;
  const ss = clamp01(s / 100);
  const ll = clamp01(l / 100);
  const c = (1 - Math.abs(2 * ll - 1)) * ss;
  const x = c * (1 - Math.abs(((hh / 60) % 2) - 1));
  const m = ll - c / 2;
  let r = 0;
  let g = 0;
  let b = 0;
  if (hh < 60) {
    r = c;
    g = x;
  } else if (hh < 120) {
    r = x;
    g = c;
  } else if (hh < 180) {
    g = c;
    b = x;
  } else if (hh < 240) {
    g = x;
    b = c;
  } else if (hh < 300) {
    r = x;
    b = c;
  } else {
    r = c;
    b = x;
  }
  const to = (v: number) =>
    Math.round((v + m) * 255)
      .toString(16)
      .padStart(2, "0");
  return `#${to(r)}${to(g)}${to(b)}`;
}

export function createDynamicSeed(): string {
  try {
    if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
      return crypto.randomUUID();
    }
  } catch {
    // ignore
  }
  return `astro-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

/**
 * 根据种子与 tab 生成双色点渐变。
 * 饱和度/亮度限制在可读区间，避免脏色与低对比。
 */
export function dynamicGradientForTab(
  seed: string,
  tabId: string,
  theme: "light" | "dark" = "light",
): ShellGradient {
  const h0 = hashString(`${seed}::${tabId}`);
  const h1 = hashString(`${seed}::${tabId}::b`);
  const hue = h0 % 360;
  const delta = 28 + (h1 % 53); // 28–80°
  const hue2 = (hue + delta) % 360;

  const sat1 = theme === "light" ? 58 + (h0 % 12) : 54 + (h0 % 14);
  const sat2 = theme === "light" ? 52 + (h1 % 14) : 48 + (h1 % 16);
  const lit1 = theme === "light" ? 46 + (h0 % 8) : 56 + (h0 % 10);
  const lit2 = theme === "light" ? 52 + (h1 % 10) : 62 + (h1 % 8);

  const primary = hslToHex(hue, sat1, lit1);
  const secondary = hslToHex(hue2, sat2, lit2);

  const px = 12 + (h0 % 28);
  const py = 6 + (h1 % 22);
  const sx = 62 + ((h0 >>> 8) % 30);
  const sy = 18 + ((h1 >>> 8) % 28);

  return {
    id: "custom",
    primary: { color: primary, x: px, y: py },
    secondary: { color: secondary, x: sx, y: sy },
    extras: [],
  };
}

/** 供导航项内联 --tone* */
export function toneCssVarsFromHex(hex: string): Record<string, string> {
  return {
    "--tone": hex,
    "--tone-soft": `color-mix(in srgb, ${hex} 18%, transparent)`,
    "--tone-glow": `color-mix(in srgb, ${hex} 35%, transparent)`,
  };
}
