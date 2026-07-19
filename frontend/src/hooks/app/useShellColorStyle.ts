/** Shell 色彩风格：多彩（随 tab 变色）/ 统一（锁定 brand blue）。 */
import { useCallback, useState } from "react";

export type ShellColorStyle = "colorful" | "unified";

const STORAGE_KEY = "astro-shell-color-style";

function readStoredStyle(): ShellColorStyle {
  try {
    const v = localStorage.getItem(STORAGE_KEY);
    if (v === "colorful" || v === "unified") return v;
  } catch {
    // ignore
  }
  return "colorful";
}

function persistStyle(style: ShellColorStyle) {
  try {
    localStorage.setItem(STORAGE_KEY, style);
  } catch {
    // ignore
  }
}

export function useShellColorStyle() {
  const [colorStyle, setColorStyleState] = useState<ShellColorStyle>(() =>
    typeof window === "undefined" ? "colorful" : readStoredStyle(),
  );

  const setColorStyle = useCallback((next: ShellColorStyle) => {
    setColorStyleState(next);
    persistStyle(next);
  }, []);

  return { colorStyle, setColorStyle };
}
