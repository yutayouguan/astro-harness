/** 窗口缩放 / 还原：瞬间贴齐工作区（不走原生 zoom 动画，避免白边）。 */
import {
  currentMonitor,
  getCurrentWindow,
  PhysicalPosition,
  PhysicalSize,
  primaryMonitor,
} from "@tauri-apps/api/window";

type Rect = { x: number; y: number; w: number; h: number };

const SNAP_EPS = 24;

type ZoomStore = Window & { __astroPrevRect?: Rect };

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

async function applyRect(rect: Rect) {
  const win = getCurrentWindow();
  document.documentElement.classList.add("zooming");
  try {
    await Promise.all([
      win.setPosition(new PhysicalPosition(rect.x, rect.y)),
      win.setSize(new PhysicalSize(Math.max(1, rect.w), Math.max(1, rect.h))),
    ]);
  } finally {
    window.setTimeout(() => {
      document.documentElement.classList.remove("zooming");
    }, 30);
  }
}

/**
 * 双击标题栏：铺满工作区（无动画瞬间切换）；再双击还原。
 * Option/Alt + 双击：系统真正最大化（原生动画已在 Rust 侧关掉）。
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
    const prev = store.__astroPrevRect;
    store.__astroPrevRect = undefined;
    await applyRect(prev);
  } else {
    store.__astroPrevRect = current;
    await applyRect(wa);
  }
}
