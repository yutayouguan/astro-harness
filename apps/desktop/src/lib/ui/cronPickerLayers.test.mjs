import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const files = await Promise.all(
  [
    "../../components/ui/Overlay.tsx",
    "../../components/ui/SelectMenu.tsx",
    "../../components/schedule/GlassDatePicker.tsx",
    "../../components/schedule/GlassTimePicker.tsx",
    "../../styles/features/cron/create-drawer.css",
  ].map((path) => readFile(new URL(path, import.meta.url), "utf8")),
);

const [overlay, selectMenu, datePicker, timePicker, drawerStyles] = files;

test("drawer preserves a feature backdrop class and cron keeps it visually transparent", () => {
  assert.match(overlay, /backdropClassName = ""/);
  assert.match(
    overlay,
    /ui-overlay--drawer-\$\{side\} \$\{backdropClassName\}/,
  );
  assert.match(drawerStyles, /\.ui-overlay\.cron-create-drawer-backdrop\s*\{/);
  assert.match(drawerStyles, /background:\s*transparent/);
  assert.match(drawerStyles, /backdrop-filter:\s*none/);
});

test("drawer-owned select, date, and time portals claim runtime overlay layers", () => {
  for (const source of [selectMenu, datePicker, timePicker]) {
    assert.match(source, /useDynamicOverlayLayer\(open\)/);
    assert.match(source, /zIndex:\s*layer/);
    assert.match(source, /onPointerDownCapture=\{bringToFront\}/);
  }
});
