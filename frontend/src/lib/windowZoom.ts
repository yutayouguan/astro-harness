/** 窗口缩放 / 还原辅助。 */
import {
  currentMonitor,
  getCurrentWindow,
  LogicalPosition,
  LogicalSize,
  primaryMonitor,
} from "@tauri-apps/api/window";
import { syncWebviewToWindow, syncWebviewToWindowSoon } from "./webviewSync";

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

/**
 * 逻辑外框矩形（位置用 outer；尺寸换算成 setSize 所需的 inner）。
 * Overlay 标题栏下 chrome 通常为 0，但显式扣除可避免把外框尺寸喂给 setSize。
 */
async function currentFrameRect(): Promise<Rect> {
  const win = getCurrentWindow();
  const [pos, outer, inner] = await Promise.all([
    win.outerPosition(),
    win.outerSize(),
    win.innerSize(),
  ]);
  const factor = await win.scaleFactor();
  const chromeW = Math.max(0, (outer.width - inner.width) / factor);
  const chromeH = Math.max(0, (outer.height - inner.height) / factor);
  return {
    x: pos.x / factor,
    y: pos.y / factor,
    w: outer.width / factor - chromeW,
    h: outer.height / factor - chromeH,
  };
}

async function workAreaInnerRect(): Promise<Rect | null> {
  const mon = (await currentMonitor()) ?? (await primaryMonitor());
  if (!mon) return null;
  const win = getCurrentWindow();
  const [outer, inner] = await Promise.all([win.outerSize(), win.innerSize()]);
  const factor = await win.scaleFactor();
  const chromeW = Math.max(0, (outer.width - inner.width) / factor);
  const chromeH = Math.max(0, (outer.height - inner.height) / factor);
  const wa = await toLogicalRect({
    x: mon.workArea.position.x,
    y: mon.workArea.position.y,
    width: mon.workArea.size.width,
    height: mon.workArea.size.height,
  });
  return {
    x: wa.x,
    y: wa.y,
    w: Math.max(1, wa.w - chromeW),
    h: Math.max(1, wa.h - chromeH),
  };
}

/** 逐帧插值位置+尺寸；每帧强制 webview 跟窗，避免 macOS 白边。 */
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
      const w = Math.round(from.w + (to.w - from.w) * e);
      const h = Math.round(from.h + (to.h - from.h) * e);
      await Promise.all([
        win.setPosition(new LogicalPosition(x, y)),
        win.setSize(new LogicalSize(Math.max(1, w), Math.max(1, h))),
      ]);
      await syncWebviewToWindow();
      if (t >= 1) break;
    }
  } finally {
    syncWebviewToWindowSoon();
    window.setTimeout(() => {
      document.documentElement.classList.remove("zooming");
      void syncWebviewToWindow();
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
    syncWebviewToWindowSoon();
    return;
  }

  const wa = await workAreaInnerRect();
  if (!wa) {
    await win.toggleMaximize();
    syncWebviewToWindowSoon();
    return;
  }

  const current = await currentFrameRect();
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
