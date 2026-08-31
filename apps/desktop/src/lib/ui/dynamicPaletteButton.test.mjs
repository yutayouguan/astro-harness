import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const component = await readFile(
  new URL("../../components/ui/DynamicPaletteButton.tsx", import.meta.url),
  "utf8",
);
const app = await readFile(new URL("../../App.tsx", import.meta.url), "utf8");
const styles = await readFile(
  new URL("../../styles/features/shell/shell.css", import.meta.url),
  "utf8",
);

test("dynamic palette button is limited to the main chat surface", () => {
  assert.match(app, /className=\{`chat-main\$\{colorStyle === "dynamic"/);
  assert.match(app, /\{colorStyle === "dynamic" \? \(/);
  assert.match(app, /onReshuffle=\{reshuffleDynamic\}/);
  assert.match(app, /label=\{t\("prefs\.colorStyle\.reshuffle"\)\}/);
});

test("dynamic seed changes crossfade from the current shell palette", () => {
  assert.match(app, /prevDynamicSeedRef\s*=\s*useRef\(dynamicSeed\)/);
  assert.match(
    app,
    /prevDynamicSeedRef\.current\s*!==\s*dynamicSeed\s*&&\s*colorStyle\s*===\s*"dynamic"/,
  );
  assert.match(app, /getComputedStyle\(shellRef\.current\)\.background/);
  assert.match(app, /key=\{toneFade\.revision\}/);
  assert.match(
    styles,
    /shell-tone-fade-out 260ms cubic-bezier\(0\.23, 1, 0\.32, 1\)/,
  );
});

test("each activation restarts the pinwheel feedback", () => {
  assert.match(component, /setSpinRevision\(\(revision\) => revision \+ 1\)/);
  assert.match(component, /key=\{spinRevision\}/);
  assert.match(component, /data-spinning=\{spinRevision > 0 \|\| undefined\}/);
  assert.match(component, /onReshuffle\(\)/);
  assert.match(component, /shell-dynamic-palette-stem/);
  assert.equal((component.match(/data-blade=/g) ?? []).length, 4);
  assert.match(component, /data-blade="blue"/);
  assert.match(component, /data-blade="green"/);
  assert.match(component, /data-blade="cyan"/);
  assert.match(component, /data-blade="red"/);
  assert.match(
    styles,
    /shell-dynamic-palette-spin 520ms cubic-bezier\(0\.77, 0, 0\.175, 1\)/,
  );
  assert.match(styles, /\.shell-dynamic-palette-rotor\[data-spinning="true"\]/);
  assert.match(styles, /transform:\s*rotate\(720deg\)/);
});

test("dynamic palette control has accessible motion and input states", () => {
  assert.match(component, /aria-label=\{label\}/);
  assert.match(styles, /\.shell-dynamic-palette-button:focus-visible/);
  assert.match(styles, /\.shell-dynamic-palette-button:active/);
  assert.match(styles, /@media \(hover: hover\) and \(pointer: fine\)/);
  assert.match(styles, /@media \(prefers-reduced-motion: reduce\)/);
  assert.match(styles, /@media \(max-width: 640px\)[\s\S]*bottom:\s*92px;/);
});
