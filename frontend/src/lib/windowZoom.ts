/** 窗口缩放 / 还原辅助。 */
import {
  currentMonitor,
  getCurrentWindow,
  LogicalPosition,
  LogicalSize,
  primaryMonitor,
} from "@tauri-apps/api/window";

type Rect = { x: number; y: number; w: number; h: number };

const SNAP_EPS = 12;
const DURATION_MS = 280;

type ZoomStore = Window & { __astroPrevRect?: Rect };

function easeOutCubic(t: number) {
  return 1 - Math.pow(1 - t, 3);
}

function near(a: number, b: number, eps = SNAP_EPS) {
  return Math.abs(a - b) <= eps;
}

async function toLogicalRect(physical: {
  x: number;
  y: number;
  width: number;
  height: number;
}): Promise<Rect> {
  const factor = await getCurrentWindow().scaleFactor();
  return {
    x: physical.x / factor,
    y: physical.y / factor,
    w: physical.width / factor,
    h: physical.height / factor,
  };
}

async function currentRect(): Promise<Rect> {
  const win = getCurrentWindow();
  const [pos, size] = await Promise.all([win.outerPosition(), win.outerSize()]);
  return toLogicalRect({
    x: pos.x,
    y: pos.y,
    width: size.width,
    height: size.height,
  });
}

async function workAreaRect(): Promise<Rect | null> {
  const mon = (await currentMonitor()) ?? (await primaryMonitor());
  if (!mon) return null;
  return toLogicalRect({
    x: mon.workArea.position.x,
    y: mon.workArea.position.y,
    width: mon.workArea.size.width,
    height: mon.workArea.size.height,
  });
}

/** 逐帧插值位置+尺寸；动画期间加 html.zooming 关掉 CSS 过渡，减轻透明窗闪白 */
async function animateRect(from: Rect, to: Rect, durationMs = DURATION_MS) {
  const win = getCurrentWindow();
  const start = performance.now();
  document.documentElement.classList.add("zooming");

  try {
    await new Promise<void>((resolve) => {
      const step = async (now: number) => {
        const t = Math.min(1, (now - start) / durationMs);
        const e = easeOutCubic(t);
        const x = Math.round(from.x + (to.x - from.x) * e);
        const y = Math.round(from.y + (to.y - from.y) * e);
        const w = Math.round(from.w + (to.w - from.w) * e);
        const h = Math.round(from.h + (to.h - from.h) * e);
        await Promise.all([
          win.setPosition(new LogicalPosition(x, y)),
          win.setSize(new LogicalSize(w, h)),
        ]);
        if (t < 1) requestAnimationFrame(step);
        else resolve();
      };
      requestAnimationFrame(step);
    });
  } finally {
    // 稍延后移除，避免最后一帧合成时闪一下
    window.setTimeout(() => {
      document.documentElement.classList.remove("zooming");
    }, 50);
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

  const wa = await workAreaRect();
  if (!wa) {
    await win.toggleMaximize();
    return;
  }

  const current = await currentRect();
  const nearWorkArea =
    near(current.x, wa.x) &&
    near(current.y, wa.y) &&
    near(current.w, wa.w, 20) &&
    near(current.h, wa.h, 20);

  const store = window as unknown as ZoomStore;
  if (nearWorkArea && store.__astroPrevRect) {
    await animateRect(current, store.__astroPrevRect);
    store.__astroPrevRect = undefined;
  } else {
    store.__astroPrevRect = current;
    await animateRect(current, wa);
  }
}
