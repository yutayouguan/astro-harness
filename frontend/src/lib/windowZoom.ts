/** 窗口缩放 / 还原辅助（物理外框 + 工作区铺满）。 */
import {
  currentMonitor,
  getCurrentWindow,
  PhysicalPosition,
  PhysicalSize,
  primaryMonitor,
} from "@tauri-apps/api/window";

type Rect = { x: number; y: number; w: number; h: number };

const SNAP_EPS = 24;
const DURATION_MS = 220;

type ZoomStore = Window & { __astroPrevRect?: Rect };

function easeOutCubic(t: number) {
  return 1 - Math.pow(1 - t, 3);
}

function near(a: number, b: number, eps = SNAP_EPS) {
  return Math.abs(a - b) <= eps;
}

async function currentOuterRect(): Promise<Rect> {
  const win = getCurrentWindow();
  const [pos, size] = await Promise.all([win.outerPosition(), win.outerSize()]);
  return { x: pos.x, y: pos.y, w: size.width, h: size.height };
}

async function workAreaOuterRect(): Promise<Rect | null> {
  const mon = (await currentMonitor()) ?? (await primaryMonitor());
  if (!mon) return null;
  return {
    x: mon.workArea.position.x,
    y: mon.workArea.position.y,
    w: mon.workArea.size.width,
    h: mon.workArea.size.height,
  };
}

/**
 * 用物理像素逐帧改外框。与已验证的社区方案一致；
 * WKWebView 贴边由 Rust `on_window_event(Resized)` 强制保证。
 */
async function animateRect(from: Rect, to: Rect, durationMs = DURATION_MS) {
  const win = getCurrentWindow();
  const start = performance.now();
  document.documentElement.classList.add("zooming");

  try {
    for (;;) {
      const now = await new Promise<number>((resolve) => {
        requestAnimationFrame(resolve);
      });
      const t = Math.min(1, (now - start) / durationMs);
      const e = easeOutCubic(t);
      const x = Math.round(from.x + (to.x - from.x) * e);
      const y = Math.round(from.y + (to.y - from.y) * e);
      const w = Math.max(1, Math.round(from.w + (to.w - from.w) * e));
      const h = Math.max(1, Math.round(from.h + (to.h - from.h) * e));
      // 不等待 IPC：并发排队会把帧序打乱；fire-and-forget 更跟手
      void win.setPosition(new PhysicalPosition(x, y));
      void win.setSize(new PhysicalSize(w, h));
      if (t >= 1) {
        await Promise.all([
          win.setPosition(new PhysicalPosition(to.x, to.y)),
          win.setSize(new PhysicalSize(to.w, to.h)),
        ]);
        break;
      }
    }
  } finally {
    window.setTimeout(() => {
      document.documentElement.classList.remove("zooming");
    }, 40);
  }
}

/**
 * 双击标题栏：铺满工作区（带动画）；再双击还原。
 * Option/Alt + 双击：系统真正最大化（无动画）。
 */
export async function zoomOrRestore(optionKey: boolean) {
  const win = getCurrentWindow();
  if (optionKey) {
    await win.toggleMaximize();
    return;
  }

  const wa = await workAreaOuterRect();
  if (!wa) {
    await win.toggleMaximize();
    return;
  }

  const current = await currentOuterRect();
  const nearWorkArea =
    near(current.x, wa.x) &&
    near(current.y, wa.y) &&
    near(current.w, wa.w, 40) &&
    near(current.h, wa.h, 40);

  const store = window as unknown as ZoomStore;
  if (nearWorkArea && store.__astroPrevRect) {
    await animateRect(current, store.__astroPrevRect);
    store.__astroPrevRect = undefined;
  } else {
    store.__astroPrevRect = current;
    await animateRect(current, wa);
  }
}
