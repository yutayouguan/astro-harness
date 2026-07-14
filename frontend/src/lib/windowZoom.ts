/** 窗口缩放 / 还原：仅自定义贴齐工作区（禁止原生 zoom）。 */
import {
  currentMonitor,
  getCurrentWindow,
  LogicalPosition,
  LogicalSize,
  primaryMonitor,
} from "@tauri-apps/api/window";

type Rect = { x: number; y: number; w: number; h: number };

const SNAP_EPS = 24;

type ZoomStore = Window & { __astroPrevRect?: Rect };

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

async function currentOuterLogical(): Promise<Rect> {
  const win = getCurrentWindow();
  const [pos, size] = await Promise.all([win.outerPosition(), win.outerSize()]);
  return toLogicalRect({
    x: pos.x,
    y: pos.y,
    width: size.width,
    height: size.height,
  });
}

async function workAreaLogical(): Promise<Rect | null> {
  const mon = (await currentMonitor()) ?? (await primaryMonitor());
  if (!mon) return null;
  return toLogicalRect({
    x: mon.workArea.position.x,
    y: mon.workArea.position.y,
    width: mon.workArea.size.width,
    height: mon.workArea.size.height,
  });
}

async function applyLogicalRect(rect: Rect) {
  const win = getCurrentWindow();
  document.documentElement.classList.add("zooming");
  try {
    await Promise.all([
      win.setPosition(new LogicalPosition(rect.x, rect.y)),
      win.setSize(
        new LogicalSize(Math.max(1, rect.w), Math.max(1, rect.h)),
      ),
    ]);
  } finally {
    window.setTimeout(() => {
      document.documentElement.classList.remove("zooming");
    }, 30);
  }
}

/** 双击标题栏：铺满 / 还原工作区。不调用原生 maximize/zoom。 */
export async function zoomOrRestore() {
  const wa = await workAreaLogical();
  if (!wa) return;

  const current = await currentOuterLogical();
  const nearWorkArea =
    near(current.x, wa.x) &&
    near(current.y, wa.y) &&
    near(current.w, wa.w, 40) &&
    near(current.h, wa.h, 40);

  const store = window as unknown as ZoomStore;
  if (nearWorkArea && store.__astroPrevRect) {
    const prev = store.__astroPrevRect;
    store.__astroPrevRect = undefined;
    await applyLogicalRect(prev);
  } else {
    store.__astroPrevRect = current;
    await applyLogicalRect(wa);
  }
}
