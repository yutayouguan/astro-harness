export type TerminalExecutionMode = "system" | "project";
export type TerminalCursorStyle = "block" | "underline" | "bar";

export type TerminalSettings = {
  executionMode: TerminalExecutionMode;
  fontFamily: string;
  fontSize: number;
  lineHeight: number;
  scrollback: number;
  cursorStyle: TerminalCursorStyle;
  cursorBlink: boolean;
};

export const TERMINAL_SETTINGS_STORAGE_KEY = "astro.terminal.settings.v1";
export const TERMINAL_SETTINGS_EVENT = "astro-terminal-settings-changed";

export const TERMINAL_FONT_PRESETS = [
  {
    label: "MesloLGS NF (Powerlevel10k)",
    value:
      '"MesloLGS NF", "Hack Nerd Font Mono", "JetBrainsMono Nerd Font", monospace',
  },
  {
    label: "Hack Nerd Font Mono",
    value:
      '"Hack Nerd Font Mono", "MesloLGS NF", "JetBrainsMono Nerd Font", monospace',
  },
  {
    label: "JetBrainsMono Nerd Font",
    value:
      '"JetBrainsMono Nerd Font", "MesloLGS NF", "Hack Nerd Font Mono", monospace',
  },
  {
    label: "macOS Mono",
    value: '"SFMono-Regular", "SF Mono", Menlo, Monaco, monospace',
  },
] as const;

export const DEFAULT_TERMINAL_SETTINGS: TerminalSettings = {
  executionMode: "system",
  fontFamily: TERMINAL_FONT_PRESETS[0].value,
  fontSize: 13,
  lineHeight: 1.25,
  scrollback: 5_000,
  cursorStyle: "bar",
  cursorBlink: true,
};

function finiteNumber(value: unknown, fallback: number, min: number, max: number) {
  const number = Number(value);
  return Number.isFinite(number) ? Math.min(max, Math.max(min, number)) : fallback;
}

export function normalizeTerminalSettings(value: unknown): TerminalSettings {
  const input = value && typeof value === "object" ? (value as Record<string, unknown>) : {};
  const executionMode = input.executionMode === "project" ? "project" : "system";
  const cursorStyle = ["block", "underline", "bar"].includes(String(input.cursorStyle))
    ? (input.cursorStyle as TerminalCursorStyle)
    : DEFAULT_TERMINAL_SETTINGS.cursorStyle;
  const fontFamily =
    typeof input.fontFamily === "string" && input.fontFamily.trim()
      ? input.fontFamily.trim().slice(0, 500)
      : DEFAULT_TERMINAL_SETTINGS.fontFamily;

  return {
    executionMode,
    fontFamily,
    fontSize: finiteNumber(input.fontSize, DEFAULT_TERMINAL_SETTINGS.fontSize, 9, 28),
    lineHeight: finiteNumber(input.lineHeight, DEFAULT_TERMINAL_SETTINGS.lineHeight, 1, 2),
    scrollback: Math.round(
      finiteNumber(input.scrollback, DEFAULT_TERMINAL_SETTINGS.scrollback, 500, 50_000),
    ),
    cursorStyle,
    cursorBlink:
      typeof input.cursorBlink === "boolean"
        ? input.cursorBlink
        : DEFAULT_TERMINAL_SETTINGS.cursorBlink,
  };
}

export function readTerminalSettings(): TerminalSettings {
  if (typeof window === "undefined") return { ...DEFAULT_TERMINAL_SETTINGS };
  try {
    const stored = window.localStorage.getItem(TERMINAL_SETTINGS_STORAGE_KEY);
    return stored
      ? normalizeTerminalSettings(JSON.parse(stored))
      : { ...DEFAULT_TERMINAL_SETTINGS };
  } catch {
    return { ...DEFAULT_TERMINAL_SETTINGS };
  }
}

export function saveTerminalSettings(value: TerminalSettings): TerminalSettings {
  const next = normalizeTerminalSettings(value);
  if (typeof window !== "undefined") {
    try {
      window.localStorage.setItem(TERMINAL_SETTINGS_STORAGE_KEY, JSON.stringify(next));
    } catch {
      // Keep the current UI session usable when WebView storage is unavailable.
    }
    window.dispatchEvent(
      new CustomEvent<TerminalSettings>(TERMINAL_SETTINGS_EVENT, { detail: next }),
    );
  }
  return next;
}

export function subscribeTerminalSettings(
  listener: (settings: TerminalSettings) => void,
): () => void {
  if (typeof window === "undefined") return () => undefined;
  const handle = (event: Event) => {
    listener(normalizeTerminalSettings((event as CustomEvent).detail));
  };
  window.addEventListener(TERMINAL_SETTINGS_EVENT, handle);
  return () => window.removeEventListener(TERMINAL_SETTINGS_EVENT, handle);
}
