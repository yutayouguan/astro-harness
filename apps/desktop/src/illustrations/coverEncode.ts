/** 将封面插画渲染为 SVG base64，写入 pending avatar。 */
import { createElement } from "react";
import { createRoot } from "react-dom/client";
import { getCoverMeta, COVER_TONE_HEX, type CoverId } from "./registry";

/**
 * 把指定封面渲染成可落盘的 SVG base64。
 */
export async function coverIllustrationToSvgBase64(
  coverId: CoverId,
  size = 256,
): Promise<string> {
  const meta = getCoverMeta(coverId);
  if (!meta) throw new Error(`Unknown cover: ${coverId}`);
  const color = COVER_TONE_HEX[meta.tone];
  const host = document.createElement("div");
  host.setAttribute("aria-hidden", "true");
  host.style.cssText =
    "position:fixed;left:-99999px;top:0;width:0;height:0;overflow:hidden;color:" +
    color;
  document.body.appendChild(host);
  const root = createRoot(host);
  try {
    root.render(
      createElement(meta.Art, {
        width: size,
        height: Math.round((size * 120) / 160),
        style: { color },
      }),
    );
    await new Promise<void>((resolve) => {
      requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
    });
    const svg = host.querySelector("svg");
    if (!svg) throw new Error("Cover SVG render failed");
    svg.setAttribute("xmlns", "http://www.w3.org/2000/svg");
    svg.setAttribute("color", color);
    if (!svg.getAttribute("width")) svg.setAttribute("width", String(size));
    const xml = new XMLSerializer().serializeToString(svg);
    return btoa(unescape(encodeURIComponent(xml)));
  } finally {
    root.unmount();
    host.remove();
  }
}
