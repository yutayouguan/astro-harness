/** Shell 统一色：预设渐变与自定义多色点。 */

export type ShellColorStyle = "colorful" | "unified";

export type ShellGradientStop = {
  color: string;
  /** 0–100，画布百分比 */
  x: number;
  /** 0–100，画布百分比 */
  y: number;
};

export type ShellGradientPresetId =
  | "ocean"
  | "aurora"
  | "indigo"
  | "violet"
  | "rose"
  | "sunset"
  | "amber"
  | "forest";

export type ShellGradient = {
  id: ShellGradientPresetId | "custom";
  primary: ShellGradientStop;
  secondary: ShellGradientStop;
  /** 用户追加的色点；最多 3 个，总色点数最多 5。 */
  extras: ShellGradientStop[];
};

export type ShellColorPrefs = {
  style: ShellColorStyle;
  gradient: ShellGradient;
};

const HEX_RE = /^#[0-9a-fA-F]{6}$/;

export function isHexColor(value: string): boolean {
  return HEX_RE.test(value);
}

export function clampPercent(n: number): number {
  if (!Number.isFinite(n)) return 50;
  return Math.min(100, Math.max(0, Math.round(n * 10) / 10));
}

export function normalizeStop(raw: unknown, fallback: ShellGradientStop): ShellGradientStop {
  if (!raw || typeof raw !== "object") return { ...fallback };
  const o = raw as Record<string, unknown>;
  const color = typeof o.color === "string" && isHexColor(o.color) ? o.color : fallback.color;
  const x = typeof o.x === "number" ? clampPercent(o.x) : fallback.x;
  const y = typeof o.y === "number" ? clampPercent(o.y) : fallback.y;
  return { color, x, y };
}

export function hexToRgb(hex: string): { r: number; g: number; b: number } | null {
  if (!isHexColor(hex)) return null;
  return {
    r: Number.parseInt(hex.slice(1, 3), 16),
    g: Number.parseInt(hex.slice(3, 5), 16),
    b: Number.parseInt(hex.slice(5, 7), 16),
  };
}

/** 两色按权重混合，返回 #RRGGBB */
export function mixHex(a: string, b: string, t = 0.5): string {
  const A = hexToRgb(a);
  const B = hexToRgb(b);
  if (!A || !B) return isHexColor(a) ? a : "#2563eb";
  const w = Math.min(1, Math.max(0, t));
  const r = Math.round(A.r + (B.r - A.r) * w);
  const g = Math.round(A.g + (B.g - A.g) * w);
  const bl = Math.round(A.b + (B.b - A.b) * w);
  return `#${[r, g, bl].map((n) => n.toString(16).padStart(2, "0")).join("")}`;
}

/** 向黑/白混色，生成 underlay 安全底色 */
export function underlayFromGradient(
  theme: "light" | "dark",
  gradient: ShellGradient,
): string {
  const colors = [
    gradient.primary.color,
    gradient.secondary.color,
    ...gradient.extras.map((stop) => stop.color),
  ];
  const mid = colors.slice(1).reduce(
    (mixed, color, index) => mixHex(mixed, color, 1 / (index + 2)),
    colors[0],
  );
  if (theme === "light") {
    return mixHex(mid, "#f8fafc", 0.72);
  }
  return mixHex(mid, "#0a1018", 0.82);
}

export const SHELL_GRADIENT_PRESETS: {
  id: ShellGradientPresetId;
  labelKey:
    | "prefs.colorStyle.preset.ocean"
    | "prefs.colorStyle.preset.aurora"
    | "prefs.colorStyle.preset.indigo"
    | "prefs.colorStyle.preset.violet"
    | "prefs.colorStyle.preset.rose"
    | "prefs.colorStyle.preset.sunset"
    | "prefs.colorStyle.preset.amber"
    | "prefs.colorStyle.preset.forest";
  primary: ShellGradientStop;
  secondary: ShellGradientStop;
}[] = [
  {
    id: "ocean",
    labelKey: "prefs.colorStyle.preset.ocean",
    primary: { color: "#2563eb", x: 18, y: 8 },
    secondary: { color: "#06b6d4", x: 88, y: 22 },
  },
  {
    id: "aurora",
    labelKey: "prefs.colorStyle.preset.aurora",
    primary: { color: "#8b5cf6", x: 16, y: 12 },
    secondary: { color: "#22d3ee", x: 86, y: 28 },
  },
  {
    id: "indigo",
    labelKey: "prefs.colorStyle.preset.indigo",
    primary: { color: "#4f46e5", x: 20, y: 10 },
    secondary: { color: "#818cf8", x: 82, y: 30 },
  },
  {
    id: "violet",
    labelKey: "prefs.colorStyle.preset.violet",
    primary: { color: "#7c3aed", x: 18, y: 14 },
    secondary: { color: "#c4b5fd", x: 84, y: 26 },
  },
  {
    id: "rose",
    labelKey: "prefs.colorStyle.preset.rose",
    primary: { color: "#db2777", x: 16, y: 10 },
    secondary: { color: "#fb7185", x: 86, y: 24 },
  },
  {
    id: "sunset",
    labelKey: "prefs.colorStyle.preset.sunset",
    primary: { color: "#ea580c", x: 18, y: 12 },
    secondary: { color: "#ef4444", x: 88, y: 28 },
  },
  {
    id: "amber",
    labelKey: "prefs.colorStyle.preset.amber",
    primary: { color: "#d97706", x: 20, y: 10 },
    secondary: { color: "#fbbf24", x: 84, y: 26 },
  },
  {
    id: "forest",
    labelKey: "prefs.colorStyle.preset.forest",
    primary: { color: "#15803d", x: 18, y: 12 },
    secondary: { color: "#34d399", x: 86, y: 24 },
  },
];

export const DEFAULT_SHELL_GRADIENT: ShellGradient = {
  id: "ocean",
  primary: { ...SHELL_GRADIENT_PRESETS[0].primary },
  secondary: { ...SHELL_GRADIENT_PRESETS[0].secondary },
  extras: [],
};

export const DEFAULT_SHELL_COLOR_PREFS: ShellColorPrefs = {
  style: "colorful",
  gradient: {
    ...DEFAULT_SHELL_GRADIENT,
    primary: { ...DEFAULT_SHELL_GRADIENT.primary },
    secondary: { ...DEFAULT_SHELL_GRADIENT.secondary },
    extras: [],
  },
};

export const SHELL_GRADIENT_SWATCH_COLORS = [
  "#ffffff",
  "#fb7185",
  "#a78bfa",
  "#ef4444",
  "#f97316",
  "#eab308",
  "#22c55e",
  "#38bdf8",
  "#2563eb",
  "#0f172a",
] as const;

export function gradientFromPreset(id: ShellGradientPresetId): ShellGradient {
  const p = SHELL_GRADIENT_PRESETS.find((x) => x.id === id) ?? SHELL_GRADIENT_PRESETS[0];
  return {
    id: p.id,
    primary: { ...p.primary },
    secondary: { ...p.secondary },
    extras: [],
  };
}

export function cloneGradient(g: ShellGradient): ShellGradient {
  return {
    id: g.id,
    primary: { ...g.primary },
    secondary: { ...g.secondary },
    extras: g.extras.map((stop) => ({ ...stop })),
  };
}

export function gradientStops(g: ShellGradient): ShellGradientStop[] {
  return [
    { ...g.primary },
    { ...g.secondary },
    ...g.extras.map((stop) => ({ ...stop })),
  ];
}

export function gradientWithStops(
  id: ShellGradient["id"],
  stops: ShellGradientStop[],
): ShellGradient {
  const safe = stops.slice(0, 5);
  while (safe.length < 2) {
    safe.push(
      safe.length === 0
        ? { ...DEFAULT_SHELL_GRADIENT.primary }
        : { ...DEFAULT_SHELL_GRADIENT.secondary },
    );
  }
  return {
    id,
    primary: { ...safe[0] },
    secondary: { ...safe[1] },
    extras: safe.slice(2).map((stop) => ({ ...stop })),
  };
}

export function gradientsEqual(a: ShellGradient, b: ShellGradient): boolean {
  return (
    a.id === b.id &&
    a.primary.color === b.primary.color &&
    a.primary.x === b.primary.x &&
    a.primary.y === b.primary.y &&
    a.secondary.color === b.secondary.color &&
    a.secondary.x === b.secondary.x &&
    a.secondary.y === b.secondary.y &&
    a.extras.length === b.extras.length &&
    a.extras.every(
      (stop, index) =>
        stop.color === b.extras[index]?.color &&
        stop.x === b.extras[index]?.x &&
        stop.y === b.extras[index]?.y,
    )
  );
}

export function gradientCssBackground(g: ShellGradient): string {
  const stops = [g.primary, g.secondary, ...g.extras];
  const radial = stops
    .map(
      (stop, index) =>
        `radial-gradient(circle at ${stop.x}% ${stop.y}%, ${stop.color}, transparent ${index === 0 ? 55 : 50}%)`,
    )
    .join(", ");
  return `${radial}, linear-gradient(135deg, ${g.primary.color}, ${g.secondary.color})`;
}

export function normalizeGradient(raw: unknown): ShellGradient {
  const fallback = cloneGradient(DEFAULT_SHELL_GRADIENT);
  if (!raw || typeof raw !== "object") return fallback;
  const o = raw as Record<string, unknown>;
  const idRaw = typeof o.id === "string" ? o.id : "ocean";
  const id =
    idRaw === "custom" || SHELL_GRADIENT_PRESETS.some((p) => p.id === idRaw)
      ? (idRaw as ShellGradient["id"])
      : "ocean";
  const extras = Array.isArray(o.extras)
    ? o.extras
        .slice(0, 3)
        .map((stop, index) =>
          normalizeStop(stop, {
            color: SHELL_GRADIENT_SWATCH_COLORS[(index + 2) % SHELL_GRADIENT_SWATCH_COLORS.length],
            x: 50,
            y: 50,
          }),
        )
    : [];
  return {
    id,
    primary: normalizeStop(o.primary, fallback.primary),
    secondary: normalizeStop(o.secondary, fallback.secondary),
    extras,
  };
}

/** 将渐变写到元素 CSS 变量，供 shell / unified tone 使用 */
export function applyShellGradientVars(
  el: HTMLElement,
  gradient: ShellGradient,
): void {
  const p = hexToRgb(gradient.primary.color);
  const s = hexToRgb(gradient.secondary.color);
  if (!p || !s) return;
  el.style.setProperty("--shell-grad-pr", String(p.r));
  el.style.setProperty("--shell-grad-pg", String(p.g));
  el.style.setProperty("--shell-grad-pb", String(p.b));
  el.style.setProperty("--shell-grad-sr", String(s.r));
  el.style.setProperty("--shell-grad-sg", String(s.g));
  el.style.setProperty("--shell-grad-sb", String(s.b));
  el.style.setProperty("--shell-grad-px", `${clampPercent(gradient.primary.x)}%`);
  el.style.setProperty("--shell-grad-py", `${clampPercent(gradient.primary.y)}%`);
  el.style.setProperty("--shell-grad-sx", `${clampPercent(gradient.secondary.x)}%`);
  el.style.setProperty("--shell-grad-sy", `${clampPercent(gradient.secondary.y)}%`);
  gradient.extras.slice(0, 3).forEach((stop, index) => {
    const rgb = hexToRgb(stop.color);
    if (!rgb) return;
    const n = index + 1;
    el.style.setProperty(`--shell-grad-e${n}r`, String(rgb.r));
    el.style.setProperty(`--shell-grad-e${n}g`, String(rgb.g));
    el.style.setProperty(`--shell-grad-e${n}b`, String(rgb.b));
    el.style.setProperty(`--shell-grad-e${n}x`, `${clampPercent(stop.x)}%`);
    el.style.setProperty(`--shell-grad-e${n}y`, `${clampPercent(stop.y)}%`);
    el.style.setProperty(`--shell-grad-e${n}a`, "1");
  });
  for (let index = gradient.extras.length; index < 3; index += 1) {
    el.style.setProperty(`--shell-grad-e${index + 1}a`, "0");
  }
  el.style.setProperty("--unified-tone", gradient.primary.color);
  el.style.setProperty(
    "--unified-tone-soft",
    `color-mix(in srgb, ${gradient.primary.color} 18%, transparent)`,
  );
  el.style.setProperty(
    "--unified-tone-glow",
    `color-mix(in srgb, ${gradient.primary.color} 35%, transparent)`,
  );
}

export function clearShellGradientVars(el: HTMLElement): void {
  for (const key of [
    "--shell-grad-pr",
    "--shell-grad-pg",
    "--shell-grad-pb",
    "--shell-grad-sr",
    "--shell-grad-sg",
    "--shell-grad-sb",
    "--shell-grad-px",
    "--shell-grad-py",
    "--shell-grad-sx",
    "--shell-grad-sy",
    "--shell-grad-e1r",
    "--shell-grad-e1g",
    "--shell-grad-e1b",
    "--shell-grad-e1x",
    "--shell-grad-e1y",
    "--shell-grad-e1a",
    "--shell-grad-e2r",
    "--shell-grad-e2g",
    "--shell-grad-e2b",
    "--shell-grad-e2x",
    "--shell-grad-e2y",
    "--shell-grad-e2a",
    "--shell-grad-e3r",
    "--shell-grad-e3g",
    "--shell-grad-e3b",
    "--shell-grad-e3x",
    "--shell-grad-e3y",
    "--shell-grad-e3a",
    "--unified-tone",
    "--unified-tone-soft",
    "--unified-tone-glow",
  ]) {
    el.style.removeProperty(key);
  }
}
