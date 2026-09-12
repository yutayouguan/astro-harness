import assert from "node:assert/strict";
import { test } from "node:test";
import { readFile } from "node:fs/promises";
import {
  INPUT_FOCUS_CONTROL,
  INPUT_FOCUS_SURFACES,
  installInputFocus,
  isFocusNavigation,
  resolveInputFocusSurface,
  prepareInputFocusBox,
} from "./inputFocus.ts";

test("only Tab navigation strengthens the input ring, not typing or IME", () => {
  const base = {
    key: "Tab",
    altKey: false,
    ctrlKey: false,
    metaKey: false,
    isComposing: false,
  };
  assert.equal(isFocusNavigation(base), true);
  for (const key of ["a", "中", "ArrowLeft", "Enter", "Escape", "Shift"]) {
    assert.equal(isFocusNavigation({ ...base, key }), false);
  }
  for (const modifier of ["altKey", "ctrlKey", "metaKey", "isComposing"]) {
    assert.equal(isFocusNavigation({ ...base, [modifier]: true }), false);
  }
});

class FakeElement {
  attributes = new Map<string, string>();
  dataset: Record<string, string> = {};
  isInput: boolean;
  composer: boolean;
  parent: FakeElement | null;
  constructor(
    isInput = true,
    composer = false,
    parent: FakeElement | null = null,
  ) {
    this.isInput = isInput;
    this.composer = composer;
    this.parent = parent;
  }
  matches(selector: string) {
    return selector === ".composer-input" ? this.composer : this.isInput;
  }
  closest(selector: string) {
    assert.ok(
      selector === INPUT_FOCUS_SURFACES ||
        selector === ".composer:not(.has-clarify)",
    );
    return this.parent;
  }
  setAttribute(name: string, value: string) {
    this.attributes.set(name, value);
  }
  removeAttribute(name: string) {
    this.attributes.delete(name);
  }
}

test("standalone inputs own focus; search and composer controls use their shell", () => {
  const shell = new FakeElement(false);
  for (const composer of [false, true]) {
    const input = new FakeElement(true, composer, shell);
    assert.equal(
      resolveInputFocusSurface(input as unknown as HTMLElement),
      shell,
    );
    input.parent = null;
    assert.equal(
      resolveInputFocusSurface(input as unknown as HTMLElement),
      input,
    );
  }
});

test("focus lifecycle moves one marker, preserves pointer typing and cleans listeners", (t) => {
  const original = Object.getOwnPropertyDescriptor(globalThis, "HTMLElement");
  Object.defineProperty(globalThis, "HTMLElement", {
    configurable: true,
    value: FakeElement,
  });
  t.after(() => {
    if (original) Object.defineProperty(globalThis, "HTMLElement", original);
    else Reflect.deleteProperty(globalThis, "HTMLElement");
  });
  const listeners = new Map<string, (event: object) => void>();
  const root = new FakeElement(false);
  const shell = new FakeElement(false);
  const input = new FakeElement(true, true, shell);
  const doc = {
    documentElement: root,
    activeElement: input,
    addEventListener(
      name: string,
      handler: (event: object) => void,
      capture: boolean,
    ) {
      assert.equal(capture, true);
      listeners.set(name, handler);
    },
    removeEventListener(
      name: string,
      handler: (event: object) => void,
      capture: boolean,
    ) {
      assert.equal(capture, true);
      assert.equal(listeners.get(name), handler);
      listeners.delete(name);
    },
  };
  const cleanup = installInputFocus(doc as unknown as Document);
  assert.equal(root.dataset.focusModality, "pointer");
  assert.ok(shell.attributes.has("data-input-focus"));
  assert.ok(input.attributes.has("data-input-focus-control"));
  assert.equal(input.attributes.has("data-input-focus"), false);
  listeners.get("keydown")?.({ key: "Tab" });
  assert.equal(root.dataset.focusModality, "keyboard");
  listeners.get("pointerdown")?.({});
  listeners.get("keydown")?.({ key: "a" });
  assert.equal(root.dataset.focusModality, "pointer");
  const standalone = new FakeElement();
  listeners.get("focusin")?.({ target: standalone });
  assert.equal(shell.attributes.size, 0);
  assert.ok(standalone.attributes.has("data-input-focus"));
  listeners.get("focusout")?.({});
  assert.equal(standalone.attributes.size, 0);
  listeners.get("focusin")?.({ target: new FakeElement(false) });
  assert.equal(root.attributes.size, 0);
  cleanup();
  assert.equal(listeners.size, 0);
  assert.equal(root.dataset.focusModality, undefined);
  // StrictMode remount must not retain a keyboard state or old subscriptions.
  installInputFocus(doc as unknown as Document)();
  assert.equal(listeners.size, 0);
  assert.equal(input.attributes.size, 0);
});

test("shared focus uses theme gradients in both materials, with contrast and solid fallbacks", async () => {
  const css = await readFile(
    new URL("../../styles/materials/soft-focus.css", import.meta.url),
    "utf8",
  );
  assert.match(css, /--input-focus-width: 1px/);
  assert.match(
    css,
    /data-focus-modality="keyboard"[\s\S]*?--input-focus-width: 2px/,
  );
  assert.match(css, /outline-offset: calc\(-1 \* var\(--input-focus-width\)\)/);
  assert.match(css, /box-shadow: none/);
  assert.match(css, /@supports \(background-clip: border-area\)/);
  assert.match(css, /background-clip: border-area, padding-box/);
  assert.match(css, /linear-gradient\(120deg/);
  assert.doesNotMatch(css, /data-material/);
  assert.doesNotMatch(
    css,
    /(?:^|[;{])\s*(?:border-width|padding|height|width):|!important/,
  );
  assert.match(css, /prefers-contrast: more/);
  assert.match(css, /aria-invalid/);
  for (const excluded of [
    "checkbox",
    "radio",
    "range",
    "file",
    "hidden",
    "color",
  ]) {
    assert.ok(INPUT_FOCUS_CONTROL.includes(`[type="${excluded}"]`));
  }
  for (const shell of [
    ".expandable-search-field",
    ".sidebar-settings-search",
    ".cron-sched-stepper",
  ]) {
    assert.ok(INPUT_FOCUS_SURFACES.includes(shell));
  }
});

test("gradient focus borrows border space from padding and restores inline values", () => {
  const values = new Map([["--input-focus-box-top", "17px"]]);
  const attributes = new Set<string>();
  const surface = {
    style: {
      getPropertyValue: (key: string) => values.get(key) ?? "",
      getPropertyPriority: () => "",
      setProperty: (key: string, value: string) => {
        values.set(key, value);
      },
      removeProperty: (key: string) => values.delete(key),
    },
    setAttribute: (key: string) => attributes.add(key),
    removeAttribute: (key: string) => attributes.delete(key),
  };
  const view = {
    getComputedStyle: () => ({
      getPropertyValue: (key: string) =>
        key.startsWith("padding") ? "12px" : "0.5px",
    }),
  };
  const restore = prepareInputFocusBox(
    surface as unknown as HTMLElement,
    view as unknown as Window,
  );
  assert.equal(values.get("--input-focus-box-top"), "12.5px");
  assert.equal(values.get("--input-focus-box-budget"), "12.5px");
  assert.ok(attributes.has("data-input-focus-box"));
  restore?.();
  assert.deepEqual([...values], [["--input-focus-box-top", "17px"]]);
  assert.equal(attributes.size, 0);
  const bareView = {
    getComputedStyle: () => ({ getPropertyValue: () => "0px" }),
  };
  assert.equal(
    prepareInputFocusBox(
      surface as unknown as HTMLElement,
      bareView as unknown as Window,
    ),
    undefined,
  );
  assert.equal(
    prepareInputFocusBox(surface as unknown as HTMLElement, null),
    undefined,
  );
});

test("bounded focus palette maintains 3:1 on soft surfaces across every hue", async () => {
  const css = await readFile(
    new URL("../../styles/materials/soft-focus.css", import.meta.url),
    "utf8",
  );
  const material = await readFile(
    new URL("../../styles/materials/soft.css", import.meta.url),
    "utf8",
  );
  const hexLuminance = (hex: string) => {
    const [r, g, b] = hex.match(/\w\w/g)!.map((part) => {
      const n = parseInt(part, 16) / 255;
      return n <= 0.04045 ? n / 12.92 : ((n + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
  };
  // CSS Color 4 OKLab inverse transform, kept local to this color-contract test.
  const oklchLuminance = (L: number, C: number, hue: number) => {
    const a = C * Math.cos((hue * Math.PI) / 180);
    const b = C * Math.sin((hue * Math.PI) / 180);
    const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3;
    const m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3;
    const s = (L - 0.0894841775 * a - 1.291485548 * b) ** 3;
    const rgb = [
      4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
      -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
      -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
    ];
    assert.ok(
      rgb.every((n) => n >= 0 && n <= 1),
      "focus colors stay in sRGB gamut",
    );
    return rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722;
  };
  assert.match(css, /var\(--input-focus-lightness\) 0\.035 h/);
  assert.match(css, /--input-focus-lightness: 0\.62/);
  assert.match(css, /--input-focus-end-lightness: 0\.60/);
  assert.doesNotMatch(css, /var\(--input-focus-color\) 80%, var\(--ink\)/);
  for (const mode of ["light", "dark"]) {
    const block = (source: string) =>
      source.match(new RegExp(`\\[data-theme="${mode}"\\] \\{([^}]+)`))![1];
    const lightness = ["lightness", "end-lightness"].map((name) =>
      Number(
        block(css).match(new RegExp(`--input-focus-${name}: ([\\d.]+)`))![1],
      ),
    );
    const fallback = block(css).match(
      /--input-focus-fallback: #([0-9a-f]{6})/,
    )![1];
    for (const surface of ["base", "surface", "inset"]) {
      const bg = hexLuminance(
        block(material).match(
          new RegExp(`--soft-${surface}: #([0-9a-f]{6})`),
        )![1],
      );
      const foregrounds = [
        hexLuminance(fallback),
        ...lightness.flatMap((L) =>
          Array.from({ length: 72 }, (_, h) => oklchLuminance(L, 0.035, h * 5)),
        ),
      ];
      for (const fg of foregrounds) {
        assert.ok(
          (Math.max(bg, fg) + 0.05) / (Math.min(bg, fg) + 0.05) >= 3,
          `${mode}/${surface}`,
        );
      }
    }
  }
});
