/** 将 title 升级为可定位的美化 tip（body 门户）。 */
import { useEffect } from "react";
import {
  clampFloatingTip,
  resolveClipBounds,
  type TipSide,
} from "../../lib/ui/clampPopover";

const GAP = 10;
const HOST_ID = "astro-ui-tip-host";
const EDGE = 12;
/** 箭头距 tip 圆角边缘的最小内边距，避免圆润箭头压到胶囊圆角 */
const ARROW_INSET = 14;

let showRaf = 0;

function getTipHost(): HTMLElement {
  let host = document.getElementById(HOST_ID);
  if (!host) {
    host = document.createElement("div");
    host.id = HOST_ID;
    host.setAttribute("aria-hidden", "true");
    document.body.appendChild(host);
  }
  return host;
}

function resolvePos(el: HTMLElement): TipSide {
  const explicit = el.getAttribute("data-tip-pos");
  if (
    explicit === "bottom" ||
    explicit === "left" ||
    explicit === "right" ||
    explicit === "top"
  ) {
    return explicit;
  }
  // 行尾操作按钮：优先左侧，避免贴右缘时 tip 盖住按钮导致 hover 粘滞
  if (el.classList.contains("ws-file-delete") || el.closest(".ws-file-delete, .fs-more-btn")) {
    return "left";
  }
  if (el.closest(".sidebar")) return "right";
  if (
    el.closest(
      ".content-header, .app-header, .topbar, .ws-toolbar, .fs-toolbar, .memory-editor-actions",
    )
  ) {
    return "bottom";
  }
  return "top";
}

function placeTip(el: HTMLElement, text: string) {
  const host = getTipHost();
  let tipEl = host.querySelector<HTMLElement>(".ui-tip");
  if (!tipEl) {
    tipEl = document.createElement("span");
    tipEl.className = "ui-tip";
    host.appendChild(tipEl);
  }

  if (showRaf) {
    cancelAnimationFrame(showRaf);
    showRaf = 0;
  }

  tipEl.textContent = text;
  tipEl.classList.toggle("is-wrap", text.length > 18);
  tipEl.style.position = "fixed";
  tipEl.style.left = "0";
  tipEl.style.top = "0";
  tipEl.style.width = "max-content";
  tipEl.style.whiteSpace = text.length > 18 ? "normal" : "nowrap";
  tipEl.style.maxWidth = "min(280px, calc(100vw - 16px))";
  tipEl.style.visibility = "hidden";
  tipEl.style.opacity = "0";
  tipEl.style.transform = "none";
  tipEl.style.transition = "none";
  tipEl.dataset.show = "0";
  delete tipEl.dataset.pos;

  // 强制同步布局后再量尺寸，避免 transition/transform 把 tipRect 量成 0
  void tipEl.offsetWidth;
  const tipW = Math.max(tipEl.offsetWidth, tipEl.getBoundingClientRect().width);
  const tipH = Math.max(tipEl.offsetHeight, tipEl.getBoundingClientRect().height);
  const rect = el.getBoundingClientRect();
  const placed = clampFloatingTip({
    anchorRect: rect,
    tipSize: { width: tipW, height: tipH },
    bounds: resolveClipBounds(el, {
      width: window.innerWidth,
      height: window.innerHeight,
    }),
    prefer: resolvePos(el),
    gap: GAP,
    pad: EDGE,
    arrowInset: ARROW_INSET,
  });

  tipEl.style.left = `${Math.round(placed.left)}px`;
  tipEl.style.top = `${Math.round(placed.top)}px`;
  tipEl.style.setProperty("--tip-arrow-x", `${Math.round(placed.arrowX)}px`);
  tipEl.style.setProperty("--tip-arrow-y", `${Math.round(placed.arrowY)}px`);
  tipEl.style.visibility = "";
  tipEl.style.opacity = "";
  tipEl.style.transform = "";
  tipEl.style.transition = "";
  tipEl.dataset.pos = placed.side;
  // 下一帧再显示，让最终坐标生效后再播入场动画
  showRaf = requestAnimationFrame(() => {
    showRaf = 0;
    tipEl.dataset.show = "1";
  });
}

function hideTip() {
  if (showRaf) {
    cancelAnimationFrame(showRaf);
    showRaf = 0;
  }
  const tipEl = document.getElementById(HOST_ID)?.querySelector<HTMLElement>(".ui-tip");
  if (!tipEl) return;
  tipEl.dataset.show = "0";
  delete tipEl.dataset.pos;
  tipEl.style.removeProperty("--tip-arrow-x");
  tipEl.style.removeProperty("--tip-arrow-y");
}

/**
 * 把原生 title 升成可美化的 data-tip，并保留 aria-label。
 * tip 渲染到 body 门户，避免 backdrop-filter / overflow 裁切。
 */
export function useBeautifyTips(enabled = true) {
  useEffect(() => {
    if (!enabled || typeof document === "undefined") return;

    const upgradeOne = (el: HTMLElement) => {
      const tip = el.getAttribute("title")?.trim() || el.getAttribute("data-tip")?.trim();
      if (!tip) return;

      if (!el.getAttribute("data-tip")) el.setAttribute("data-tip", tip);
      if (!el.getAttribute("aria-label")) el.setAttribute("aria-label", tip);
      if (el.hasAttribute("title")) el.removeAttribute("title");

      if (!el.hasAttribute("data-tip-pos")) {
        if (el.classList.contains("ws-file-delete") || el.closest(".ws-file-delete, .fs-more-btn")) {
          el.setAttribute("data-tip-pos", "left");
        } else if (el.closest(".sidebar")) {
          el.setAttribute("data-tip-pos", "right");
        } else if (
          el.closest(
            ".content-header, .app-header, .topbar, .ws-toolbar, .fs-toolbar, .memory-editor-actions",
          )
        ) {
          el.setAttribute("data-tip-pos", "bottom");
        }
      }
    };

    const upgrade = (root: ParentNode = document.body) => {
      if (root instanceof HTMLElement) {
        if (root.hasAttribute("title") || root.hasAttribute("data-tip")) upgradeOne(root);
      }
      root.querySelectorAll<HTMLElement>("[title], [data-tip]").forEach(upgradeOne);
    };

    let activeEl: HTMLElement | null = null;

    const showFor = (el: HTMLElement) => {
      const tipText = el.getAttribute("data-tip")?.trim();
      if (!tipText) return;
      activeEl = el;
      placeTip(el, tipText);
    };

    const hideIfActive = (el: HTMLElement | null) => {
      if (!el || el !== activeEl) return;
      activeEl = null;
      hideTip();
    };

    // mouseover/out 会冒泡，比 pointerenter/leave 更适合委托；用 relatedTarget 避免子节点间误触发
    const onOver = (e: Event) => {
      const ne = e as MouseEvent;
      const el = (ne.target as HTMLElement | null)?.closest?.("[data-tip]") as HTMLElement | null;
      if (!el) return;
      const from = ne.relatedTarget as Node | null;
      if (from && el.contains(from)) return;
      showFor(el);
    };

    const onOut = (e: Event) => {
      const ne = e as MouseEvent;
      const el = (ne.target as HTMLElement | null)?.closest?.("[data-tip]") as HTMLElement | null;
      if (!el) return;
      const to = ne.relatedTarget as Node | null;
      if (to && el.contains(to)) return;
      hideIfActive(el);
    };

    const onFocusIn = (e: Event) => {
      const el = (e.target as HTMLElement | null)?.closest?.("[data-tip]") as HTMLElement | null;
      if (!el) return;
      showFor(el);
    };

    const onFocusOut = (e: Event) => {
      const el = (e.target as HTMLElement | null)?.closest?.("[data-tip]") as HTMLElement | null;
      hideIfActive(el);
    };

    const onScrollOrResize = () => {
      if (!activeEl) return;
      const tipText = activeEl.getAttribute("data-tip")?.trim();
      if (!tipText) {
        activeEl = null;
        hideTip();
        return;
      }
      placeTip(activeEl, tipText);
    };

    const onPointerDown = () => {
      if (!activeEl) return;
      activeEl = null;
      hideTip();
    };

    upgrade();
    getTipHost();

    document.body.addEventListener("mouseover", onOver, true);
    document.body.addEventListener("mouseout", onOut, true);
    document.body.addEventListener("focusin", onFocusIn, true);
    document.body.addEventListener("focusout", onFocusOut, true);
    document.body.addEventListener("pointerdown", onPointerDown, true);
    window.addEventListener("scroll", onScrollOrResize, true);
    window.addEventListener("resize", onScrollOrResize);
    window.addEventListener("blur", onPointerDown);

    const mo = new MutationObserver((mutations) => {
      for (const m of mutations) {
        if (m.type === "attributes") {
          const el = m.target as HTMLElement;
          if (el?.nodeType === 1) upgradeOne(el);
          continue;
        }
        if (m.type === "childList") {
          m.addedNodes.forEach((node) => {
            if (node instanceof HTMLElement) upgrade(node);
          });
        }
      }
    });

    mo.observe(document.body, {
      subtree: true,
      childList: true,
      attributes: true,
      attributeFilter: ["title", "data-tip"],
    });

    return () => {
      mo.disconnect();
      if (showRaf) {
        cancelAnimationFrame(showRaf);
        showRaf = 0;
      }
      document.body.removeEventListener("mouseover", onOver, true);
      document.body.removeEventListener("mouseout", onOut, true);
      document.body.removeEventListener("focusin", onFocusIn, true);
      document.body.removeEventListener("focusout", onFocusOut, true);
      document.body.removeEventListener("pointerdown", onPointerDown, true);
      window.removeEventListener("scroll", onScrollOrResize, true);
      window.removeEventListener("resize", onScrollOrResize);
      window.removeEventListener("blur", onPointerDown);
      document.getElementById(HOST_ID)?.remove();
    };
  }, [enabled]);
}
