/** Shell 统一色：预设渐变与自定义多色点。 */

export type ShellColorStyle = "colorful" | "unified" | "dynamic";

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
  /** 灵动配色本机种子；换种子即全体 Tab 换色 */
  dynamicSeed: string;
};

const HEX_RE = /^#[0-9a-fA-F]{6}$/;

export function isHexColor(value: string): boolean {
  return HEX_RE.test(value);
}

export function clampPercent(n: number): number {
  if (!Number.isFinite(n)) return 50;
  return Math.min(100, Math.max(0, Math.round(n * 10) / 10));
}

export function normalizeStop(
  raw: unknown,
  fallback: ShellGradientStop,
): ShellGradientStop {
  if (!raw || typeof raw !== "object") return { ...fallback };
  const o = raw as Record<string, unknown>;
  const color =
    typeof o.color === "string" && isHexColor(o.color)
      ? o.color
      : fallback.color;
  const x = typeof o.x === "number" ? clampPercent(o.x) : fallback.x;
  const y = typeof o.y === "number" ? clampPercent(o.y) : fallback.y;
  return { color, x, y };
}

export function hexToRgb(
  hex: string,
): { r: number; g: number; b: number } | null {
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
  const mid = colors
    .slice(1)
    .reduce(
      (mixed, color, index) => mixHex(mixed, color, 1 / (index + 2)),
      colors[0],
    );
  if (theme === "light") {
    if (isNearWhite(mid)) {
      return mixHex("#ffffff", "#e2e8f0", 0.28);
    }
    return mixHex(mid, "#f8fafc", 0.72);
  }
  if (isNearBlack(mid)) {
    return mixHex("#0a1018", "#1e293b", 0.22);
  }
  return mixHex(mid, "#0a1018", 0.82);
}

/** sRGB 相对亮度 0–1，用于极端白/黑检测 */
export function relativeLuminance(hex: string): number {
  const rgb = hexToRgb(hex);
  if (!rgb) return 0.5;
  const channel = (c: number) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return (
    0.2126 * channel(rgb.r) + 0.7152 * channel(rgb.g) + 0.0722 * channel(rgb.b)
  );
}

export function isNearWhite(hex: string, threshold = 0.9): boolean {
  return relativeLuminance(hex) >= threshold;
}

export function isNearBlack(hex: string, threshold = 0.1): boolean {
  return relativeLuminance(hex) <= threshold;
}

/** 统一色在亮/暗主题下的可读强调色（背景仍用用户选色） */
export type UnifiedSurfaceMode = "default" | "light-neutral" | "dark-neutral";

export function unifiedSurfaceMode(
  theme: "light" | "dark",
  primary: string,
): UnifiedSurfaceMode {
  if (theme === "light" && isNearWhite(primary)) return "light-neutral";
  if (theme === "dark" && isNearBlack(primary)) return "dark-neutral";
  return "default";
}

const UNIFIED_ACCENT_LIGHT_NEUTRAL = "#64748b";
const UNIFIED_ACCENT_DARK_NEUTRAL = "#94a3b8";

/** 壳层渐变保留用户色；UI 强调色在极端白/黑时回退为中性 slate */
export function effectiveUnifiedTone(
  theme: "light" | "dark",
  primary: string,
): string {
  const mode = unifiedSurfaceMode(theme, primary);
  if (mode === "light-neutral") return UNIFIED_ACCENT_LIGHT_NEUTRAL;
  if (mode === "dark-neutral") return UNIFIED_ACCENT_DARK_NEUTRAL;
  return primary;
}

export type ShellHaloRole = "primary" | "secondary" | "extra";

export function shellStopCount(gradient: ShellGradient): number {
  return Math.min(5, Math.max(2, 2 + gradient.extras.length));
}

/** 色点越多整体越淡，避免糊成一片；2 点略加强 */
export function shellStopCountScale(stopCount: number): number {
  const n = Math.min(5, Math.max(2, stopCount));
  // 2→1.08, 3→0.96, 4→0.84, 5→0.72
  return Math.round((1.08 - (n - 2) * 0.12) * 1000) / 1000;
}

export function shellSurfaceScale(surface: UnifiedSurfaceMode): number {
  if (surface === "light-neutral") return 0.42;
  if (surface === "dark-neutral") return 0.65;
  return 1;
}

/**
 * 全局光晕强度：色点数 × 极端白/黑表面系数。
 * 用户不调节；写入 `--shell-grad-strength`。
 */
export function shellGradStrength(
  theme: "light" | "dark",
  gradient: ShellGradient,
): number {
  const surface = unifiedSurfaceMode(theme, gradient.primary.color);
  return (
    Math.round(
      shellStopCountScale(shellStopCount(gradient)) *
        shellSurfaceScale(surface) *
        1000,
    ) / 1000
  );
}

/** 光晕扩散：色点少略大，色点多略收 */
export function shellGradSpread(stopCount: number): number {
  const n = Math.min(5, Math.max(2, stopCount));
  // 2→1.1, 3→1.0, 4→0.92, 5→0.86
  return Math.round((1.1 - (n - 2) * 0.08) * 1000) / 1000;
}

/** 角色档位：主色更实更大，辅色次之，额外点更淡更小（与壳层 CSS 基线对齐） */
export function shellHaloForRole(
  role: ShellHaloRole,
  theme: "light" | "dark",
  extraIndex = 0,
): { alpha: number; fadePct: number } {
  if (theme === "light") {
    if (role === "primary") return { alpha: 0.46, fadePct: 55 };
    if (role === "secondary") return { alpha: 0.5, fadePct: 50 };
    const extras = [
      { alpha: 0.38, fadePct: 48 },
      { alpha: 0.34, fadePct: 46 },
      { alpha: 0.3, fadePct: 44 },
    ];
    return extras[Math.min(Math.max(0, extraIndex), 2)];
  }
  if (role === "primary") return { alpha: 0.28, fadePct: 55 };
  if (role === "secondary") return { alpha: 0.26, fadePct: 50 };
  const extras = [
    { alpha: 0.2, fadePct: 48 },
    { alpha: 0.18, fadePct: 46 },
    { alpha: 0.16, fadePct: 44 },
  ];
  return extras[Math.min(Math.max(0, extraIndex), 2)];
}

/**
 * 编辑器画布预览：与壳层同一套角色 / 点数 / 主题公式。
 * 用户只改色与位置，不暴露大小/透明度滑块。
 */
export function shellGradientPreviewBackground(
  gradient: ShellGradient,
  theme: "light" | "dark",
): string {
  const stops = gradientStops(gradient);
  const strength = shellGradStrength(theme, gradient);
  const spread = shellGradSpread(stops.length);
  const base = theme === "dark" ? "#0a1018" : "#e8edf5";
  const layers = stops.map((stop, index) => {
    const role: ShellHaloRole =
      index === 0 ? "primary" : index === 1 ? "secondary" : "extra";
    const { alpha, fadePct } = shellHaloForRole(
      role,
      theme,
      Math.max(0, index - 2),
    );
    const a = Math.round(alpha * strength * 1000) / 1000;
    const fade = Math.round(fadePct * spread * 10) / 10;
    const rgb = hexToRgb(stop.color);
    const color = rgb ? `rgba(${rgb.r}, ${rgb.g}, ${rgb.b}, ${a})` : stop.color;
    return `radial-gradient(circle at ${stop.x}% ${stop.y}%, ${color} 0%, transparent ${fade}%)`;
  });
  // 壳层在底部 50% 100% 还叠一层辅色回响，预览同步，避免底部死区
  const echo = stops[1] ?? stops[0];
  if (echo) {
    const a =
      Math.round((theme === "dark" ? 0.12 : 0.22) * strength * 1000) / 1000;
    const fade = Math.round(55 * spread * 10) / 10;
    const rgb = hexToRgb(echo.color);
    const color = rgb
      ? `rgba(${rgb.r}, ${rgb.g}, ${rgb.b}, ${a})`
      : echo.color;
    layers.push(
      `radial-gradient(circle at 50% 100%, ${color} 0%, transparent ${fade}%)`,
    );
  }
  return `${layers.join(", ")}, ${base}`;
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
    primary: { color: "#3567c9", x: 20, y: 12 },
    secondary: { color: "#4fc0b8", x: 80, y: 86 },
  },
  {
    id: "aurora",
    labelKey: "prefs.colorStyle.preset.aurora",
    primary: { color: "#7250d8", x: 16, y: 10 },
    secondary: { color: "#4fcf9c", x: 84, y: 88 },
  },
  {
    id: "indigo",
    labelKey: "prefs.colorStyle.preset.indigo",
    primary: { color: "#3f4fae", x: 12, y: 20 },
    secondary: { color: "#7d97e8", x: 86, y: 72 },
  },
  {
    id: "violet",
    labelKey: "prefs.colorStyle.preset.violet",
    primary: { color: "#8a55cf", x: 22, y: 10 },
    secondary: { color: "#f2a888", x: 78, y: 86 },
  },
  {
    id: "rose",
    labelKey: "prefs.colorStyle.preset.rose",
    primary: { color: "#cf4480", x: 16, y: 14 },
    secondary: { color: "#f9b98d", x: 84, y: 80 },
  },
  {
    id: "sunset",
    labelKey: "prefs.colorStyle.preset.sunset",
    primary: { color: "#e0662b", x: 18, y: 82 },
    secondary: { color: "#d1568f", x: 82, y: 18 },
  },
  {
    id: "amber",
    labelKey: "prefs.colorStyle.preset.amber",
    primary: { color: "#cf8b22", x: 14, y: 18 },
    secondary: { color: "#34a89a", x: 86, y: 84 },
  },
  {
    id: "forest",
    labelKey: "prefs.colorStyle.preset.forest",
    primary: { color: "#3d8a58", x: 22, y: 12 },
    secondary: { color: "#d8c06a", x: 80, y: 88 },
  },
];

export const DEFAULT_SHELL_GRADIENT: ShellGradient = {
  id: "ocean",
  primary: { ...SHELL_GRADIENT_PRESETS[0].primary },
  secondary: { ...SHELL_GRADIENT_PRESETS[0].secondary },
  extras: [],
};

export const DEFAULT_SHELL_COLOR_PREFS: ShellColorPrefs = {
  style: "dynamic",
  gradient: {
    ...DEFAULT_SHELL_GRADIENT,
    primary: { ...DEFAULT_SHELL_GRADIENT.primary },
    secondary: { ...DEFAULT_SHELL_GRADIENT.secondary },
    extras: [],
  },
  dynamicSeed: "astro-default-seed",
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
  const p =
    SHELL_GRADIENT_PRESETS.find((x) => x.id === id) ??
    SHELL_GRADIENT_PRESETS[0];
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
  if (stops.length <= 2) {
    return `linear-gradient(135deg, ${g.primary.color} 8%, ${g.secondary.color} 92%)`;
  }
  const mid = Math.round(100 / (stops.length - 1));
  const parts = stops
    .map((stop, index) => `${stop.color} ${Math.min(100, index * mid)}%`)
    .join(", ");
  return `linear-gradient(135deg, ${parts})`;
}

/** 小色板预览：线性渐变更干净，避免小圆里径向叠色发浊 */
export function gradientSwatchBackground(g: ShellGradient): string {
  const stops = gradientStops(g);
  if (stops.length <= 1) return stops[0]?.color ?? "#2563eb";
  if (stops.length === 2) {
    return `linear-gradient(135deg, ${stops[0].color} 8%, ${stops[1].color} 92%)`;
  }
  const step = 100 / (stops.length - 1);
  return `linear-gradient(135deg, ${stops
    .map((stop, index) => `${stop.color} ${Math.round(index * step)}%`)
    .join(", ")})`;
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
    ? o.extras.slice(0, 3).map((stop, index) =>
        normalizeStop(stop, {
          color:
            SHELL_GRADIENT_SWATCH_COLORS[
              (index + 2) % SHELL_GRADIENT_SWATCH_COLORS.length
            ],
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
  theme: "light" | "dark" = el.getAttribute("data-theme") === "dark"
    ? "dark"
    : "light",
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
  el.style.setProperty(
    "--shell-grad-px",
    `${clampPercent(gradient.primary.x)}%`,
  );
  el.style.setProperty(
    "--shell-grad-py",
    `${clampPercent(gradient.primary.y)}%`,
  );
  el.style.setProperty(
    "--shell-grad-sx",
    `${clampPercent(gradient.secondary.x)}%`,
  );
  el.style.setProperty(
    "--shell-grad-sy",
    `${clampPercent(gradient.secondary.y)}%`,
  );
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

  const surface = unifiedSurfaceMode(theme, gradient.primary.color);
  if (surface === "default") {
    el.removeAttribute("data-unified-surface");
  } else {
    el.setAttribute("data-unified-surface", surface);
  }

  const accent = effectiveUnifiedTone(theme, gradient.primary.color);
  el.style.setProperty("--unified-tone", accent);
  el.style.setProperty(
    "--unified-tone-soft",
    `color-mix(in srgb, ${accent} 28%, transparent)`,
  );
  el.style.setProperty(
    "--unified-tone-glow",
    `color-mix(in srgb, ${accent} 40%, transparent)`,
  );
  el.style.setProperty(
    "--shell-grad-strength",
    String(shellGradStrength(theme, gradient)),
  );
  el.style.setProperty(
    "--shell-grad-spread",
    String(shellGradSpread(shellStopCount(gradient))),
  );
  el.style.setProperty(
    "--window-underlay",
    underlayFromGradient(theme, gradient),
  );
}

export function clearShellGradientVars(el: HTMLElement): void {
  el.removeAttribute("data-unified-surface");
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
    "--shell-grad-strength",
    "--shell-grad-spread",
    "--window-underlay",
  ]) {
    el.style.removeProperty(key);
  }
}
