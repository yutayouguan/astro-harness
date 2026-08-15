/**
 * macOS 标题栏缩放：社区公认解法（tauri#13898 / Sparky）。
 *
 * 原生 Overlay 标题栏双击走 `NSWindow zoom`，窗体先变大、WKWebView 晚到 → 白边。
 * 做法：禁止原生 zoom；用物理像素逐帧 setPosition/setSize，让 WebView 跟每一帧。
 */
import { PhysicalPosition, PhysicalSize } from "@tauri-apps/api/dpi";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  currentMonitor,
  getCurrentWindow,
  primaryMonitor,
} from "@tauri-apps/api/window";

type Rect = { x: number; y: number; w: number; h: number };

const DURATION_MS = 650;

let prevRect: Rect | null = null;
let pseudoMaximized = false;
let animating = false;
let cachedRect: Rect | null = null;
let cachedTarget: Rect | null = null;
let redirectInstalled = false;

function easeInOutQuart(t: number) {
  return t < 0.5 ? 8 * t * t * t * t : 1 - Math.pow(-2 * t + 2, 4) / 2;
}

async function readOuterRect(): Promise<Rect> {
  const win = getCurrentWindow();
  const [pos, size] = await Promise.all([win.outerPosition(), win.outerSize()]);
  return { x: pos.x, y: pos.y, w: size.width, h: size.height };
}

async function readWorkAreaRect(): Promise<Rect | null> {
  const mon = (await currentMonitor()) ?? (await primaryMonitor());
  if (!mon) return null;
  return {
    x: mon.workArea.position.x,
    y: mon.workArea.position.y,
    w: mon.workArea.size.width,
    h: mon.workArea.size.height,
  };
}

/** 单击时预取当前位置与目标工作区，双击时可立刻开动画。 */
export async function prefetchZoomState(): Promise<void> {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
    return;
  }
  try {
    const [rect, target] = await Promise.all([
      readOuterRect(),
      readWorkAreaRect(),
    ]);
    cachedRect = rect;
    cachedTarget = target;
  } catch {
    // ignore
  }
}

async function animatePhysical(from: Rect, to: Rect): Promise<void> {
  const win = getCurrentWindow();
  animating = true;
  document.documentElement.classList.add("zooming");
  // 动画前把原生窗/WebView 底色锁到当前 underlay，避免扩边露白
  try {
    const underlay =
      getComputedStyle(document.documentElement)
        .getPropertyValue("--window-underlay")
        .trim() || "#080e16";          // 暗色保底，比亮蓝 #dbeafe 更安全
    await Promise.all([
      win.setBackgroundColor(underlay),
      getCurrentWebview().setBackgroundColor(underlay),
    ]);
    // 再等一帧，让底色变更提交到合成器，再开始 setSize
    await new Promise<void>((r) => requestAnimationFrame(() => r()));
  } catch {
    // ignore
  }
  const start = performance.now();
  try {
    await new Promise<void>((resolve) => {
      const step = (now: number) => {
        const t = Math.min(1, (now - start) / DURATION_MS);
        const e = easeInOutQuart(t);
        const x = Math.round(from.x + (to.x - from.x) * e);
        const y = Math.round(from.y + (to.y - from.y) * e);
        const w = Math.max(1, Math.round(from.w + (to.w - from.w) * e));
        const h = Math.max(1, Math.round(from.h + (to.h - from.h) * e));
        void win.setPosition(new PhysicalPosition(x, y));
        void win.setSize(new PhysicalSize(w, h));
        if (t < 1) requestAnimationFrame(step);
        else {
          void Promise.all([
            win.setPosition(new PhysicalPosition(to.x, to.y)),
            win.setSize(new PhysicalSize(to.w, to.h)),
          ]).finally(() => resolve());
        }
      };
      requestAnimationFrame(step);
    });
  } finally {
    animating = false;
    // 稍留一会再开磨砂，等布局稳定，减少卡片闪白
    window.setTimeout(() => {
      document.documentElement.classList.remove("zooming");
    }, 120);
  }
}

/** 双击标题栏：伪最大化 / 还原（非原生 zoom）。 */
export async function zoomOrRestore(): Promise<void> {
  if (animating) return;
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
    return;
  }

  const current = cachedRect ?? (await readOuterRect());
  cachedRect = null;

  if (!pseudoMaximized) {
    const target = cachedTarget ?? (await readWorkAreaRect());
    cachedTarget = null;
    if (!target) return;
    prevRect = { ...current };
    pseudoMaximized = true;
    await animatePhysical(current, target);
  } else {
    const target = prevRect ?? current;
    prevRect = null;
    pseudoMaximized = false;
    cachedTarget = null;
    await animatePhysical(current, target);
    try {
      const win = getCurrentWindow();
      if (await win.isMaximized()) {
        await win.unmaximize();
      }
    } catch {
      // ignore
    }
  }
}

/**
 * 绿灯若走到原生 maximize：拆掉后改走伪最大化动画（与 Sparky 相同）。
 * 应用启动时调用一次即可。
 */
export function installMacMaximizeRedirect(): () => void {
  if (
    typeof window === "undefined" ||
    !("__TAURI_INTERNALS__" in window) ||
    redirectInstalled
  ) {
    return () => {};
  }
  if (!navigator.userAgent.includes("Mac")) {
    return () => {};
  }
  redirectInstalled = true;
  let unlisten: (() => void) | undefined;
  void getCurrentWindow()
    .onResized(() => {
      if (animating) return;
      void getCurrentWindow()
        .isMaximized()
        .then(async (maximized) => {
          if (maximized && !pseudoMaximized) {
            try {
              await getCurrentWindow().unmaximize();
              await zoomOrRestore();
            } catch {
              // ignore
            }
          }
        })
        .catch(() => {});
    })
    .then((fn) => {
      unlisten = fn;
    })
    .catch(() => {});
  return () => {
    unlisten?.();
    redirectInstalled = false;
  };
}

export function isZoomAnimating(): boolean {
  return animating;
}
