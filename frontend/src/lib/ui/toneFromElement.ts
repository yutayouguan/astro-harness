import type { CSSProperties } from "react";

/** 从触发元素读取 `--tone` / `--tone-soft`，供 portal 到 body 的浮层继承当前 tab 主题。 */
export function toneStyleFromElement(el: Element | null): CSSProperties {
  if (!el || typeof getComputedStyle !== "function") return {};
  const cs = getComputedStyle(el);
  const tone = cs.getPropertyValue("--tone").trim();
  const toneSoft = cs.getPropertyValue("--tone-soft").trim();
  const toneGlow = cs.getPropertyValue("--tone-glow").trim();
  const style: CSSProperties = {};
  if (tone) style["--tone" as keyof CSSProperties] = tone as never;
  if (toneSoft) style["--tone-soft" as keyof CSSProperties] = toneSoft as never;
  if (toneGlow) style["--tone-glow" as keyof CSSProperties] = toneGlow as never;
  return style;
}
