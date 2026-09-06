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
  assert.match(
    app,
    /className=\{`chat-main\$\{colorStyle === "dynamic" \|\| wallpaperEnabled/,
  );
  assert.match(app, /\{colorStyle === "dynamic" \|\| wallpaperEnabled \? \(/);
  assert.match(app, /wallpaper\.cycleRecent\(\)/);
  assert.match(app, /openSettingsTab\("preferences:appearance"\)/);
  assert.match(app, /t\("prefs\.wallpaper\.cycle"\)/);
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
  assert.match(
    component,
    /const rotorRef = useRef<SVGGElement \| null>\(null\)/,
  );
  assert.match(component, /rotor\.getAnimations\(\)\.forEach/);
  assert.match(component, /animation\.cancel\(\)/);
  assert.match(component, /rotor\.animate\(/);
  assert.match(component, /reduceMotion \? 360 : 720/);
  assert.match(component, /duration: reduceMotion \? 1320 : 1680/);
  assert.match(component, /easing: "linear"/);
  assert.match(component, /ref=\{rotorRef\}/);
  assert.match(component, /onReshuffle\(\)/);
  assert.match(component, /shell-dynamic-palette-stem/);
  assert.match(component, /d="M24 25v39\.5"/);
  assert.equal((component.match(/data-blade=/g) ?? []).length, 4);
  assert.match(component, /data-blade="blue"/);
  assert.match(component, /data-blade="green"/);
  assert.match(component, /data-blade="cyan"/);
  assert.match(component, /data-blade="red"/);
  assert.match(
    styles,
    /\.shell-dynamic-palette-button\s*\{[\s\S]*border:\s*0;/,
  );
  assert.match(
    styles,
    /\.shell-dynamic-palette-button\s*\{[\s\S]*background:\s*transparent;/,
  );
  assert.match(
    styles,
    /\.shell-dynamic-palette-rotor\s*\{[\s\S]*will-change:\s*transform;/,
  );
});

test("dynamic palette control has accessible motion and input states", () => {
  assert.match(component, /aria-label=\{label\}/);
  assert.match(styles, /\.shell-dynamic-palette-button:focus-visible/);
  assert.match(styles, /\.shell-dynamic-palette-button:active/);
  assert.match(styles, /@media \(hover: hover\) and \(pointer: fine\)/);
  assert.match(styles, /@media \(prefers-reduced-motion: reduce\)/);
  assert.match(
    styles,
    /\.shell-dynamic-palette-button\s*\{[\s\S]*bottom:\s*0;/,
  );
  assert.match(styles, /@media \(max-width: 640px\)[\s\S]*bottom:\s*0;/);
});
