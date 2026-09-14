import assert from "node:assert/strict";
import test from "node:test";
import { createWallpaperLivePreview } from "./wallpaperLivePreview.ts";

function fixture() {
  const props = new Map([
    ["--wallpaper-shade", "0.18"],
    ["--wallpaper-blur", "0"],
  ]);
  let currentPath = "one";
  let changed: (() => void) | null = null;
  const preview = createWallpaperLivePreview((path) =>
    path !== currentPath
      ? null
      : {
          style: {
            getPropertyValue: (key) => props.get(key) ?? "",
            setProperty: (key, value) => {
              props.set(key, value);
            },
            removeProperty: (key) => {
              props.delete(key);
              return "";
            },
          },
          matches: () => path === currentPath,
          observe: (callback) => {
            changed = callback;
            return () => {
              changed = null;
            };
          },
        },
  );
  return {
    props,
    preview,
    changed: () => changed?.(),
    switchPath: () => {
      currentPath = "two";
      changed?.();
    },
  };
}

test("input paints only transient image-layer values, not saved values", () => {
  const { props, preview } = fixture();
  preview.show("one", { shade: 40, blur: 8 });
  assert.equal(props.get("--wallpaper-live-shade"), "0.4");
  assert.equal(props.get("--wallpaper-live-blur"), "8");
  assert.equal(props.get("--wallpaper-shade"), "0.18");
  assert.equal(props.get("--wallpaper-blur"), "0");
});

test("slow persistence keeps the live value until the rendered base catches up", () => {
  const { props, preview, changed } = fixture();
  preview.show("one", { blur: 8 });
  preview.settle();
  assert.equal(props.get("--wallpaper-live-blur"), "8");
  props.set("--wallpaper-blur", "4");
  changed();
  assert.equal(props.get("--wallpaper-live-blur"), "8");
  props.set("--wallpaper-blur", "8");
  changed();
  assert.equal(props.has("--wallpaper-live-blur"), false);
});

test("new input is not cleared by an older acknowledgement; cancel and image switch clean up", () => {
  const { props, preview, changed, switchPath } = fixture();
  preview.show("one", { shade: 30 });
  preview.settle();
  preview.show("one", { shade: 45 });
  props.set("--wallpaper-shade", "0.3");
  changed();
  assert.equal(props.get("--wallpaper-live-shade"), "0.45");
  preview.cancel();
  assert.equal(props.has("--wallpaper-live-shade"), false);
  preview.show("one", { blur: 6 });
  switchPath();
  assert.equal(props.has("--wallpaper-live-blur"), false);
  preview.show("one", { blur: 12 });
  assert.equal(props.has("--wallpaper-live-blur"), false);
});
