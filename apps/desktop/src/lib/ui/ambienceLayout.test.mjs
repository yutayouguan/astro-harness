import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
const read = (path) => readFile(new URL(path, import.meta.url), "utf8");
const [css, panel, size] = await Promise.all([
  read("../../styles/features/desktop-ambience.css"),
  read("../../components/ui/DesktopAmbienceButton.tsx"),
  read("../../components/ui/AmbiencePetScaleControl.tsx"),
]);

test("stable popover geometry and equal tabs leave scrolling to the middle panel", () => {
  assert.match(css, /height: min\(560px, 75dvh\)/);
  assert.match(panel, /maxHeightCap=\{560\}/);
  assert.match(
    css,
    /\.ambience-content\s*\{[^}]*flex: 1;[^}]*min-height: 0;[^}]*overflow: auto;[^}]*scrollbar-gutter: stable;/,
  );
  assert.match(
    css,
    /\.desktop-ambience \.ambience-tabs\s*\{[^}]*grid-template-columns: repeat\(3, minmax\(0, 1fr\)\)/,
  );
  assert.ok(
    panel.indexOf("{scope && (") > panel.indexOf("{state.error && ("),
    "context actions stay outside the scrolling content",
  );
  assert.match(
    css,
    /@media \(max-width: 390px\)[\s\S]*grid-template-columns: repeat\(2, minmax\(0, 1fr\)\)/,
  );
});

test("pet status badges do not create a third row and size help remains accessible", () => {
  assert.match(
    css,
    /\.ambience-pets > button\s*\{[^}]*grid-template-rows: 36px auto/,
  );
  assert.match(css, /\.ambience-pet-current\s*\{[^}]*position: absolute/);
  assert.match(panel, /className="ambience-pet-picker"/);
  assert.match(size, /className="ambience-size-label"/);
  assert.match(size, /className=\{petId \? "sr-only" : "ambience-help"\}/);
  assert.match(size, /aria-describedby=\{id \+ "-hint"\}/);
});

test("random options keep their trigger anchored and avoid an extra summary row", () => {
  assert.match(css, /\.ambience-context-action\s*\{[^}]*position: relative/);
  assert.match(
    css,
    /\.ambience-more summary\s*\{[^}]*position: absolute;[^}]*top: var\(--ambience-action-inset\);[^}]*right: 0/,
  );
  assert.match(css, /width: calc\(100% - 40px\)/);
  assert.doesNotMatch(css, /\.ambience-more\[open\]\s*\{[^}]*order:/);
  assert.doesNotMatch(panel, /Only the current desktop pet's scenes/);
  assert.match(panel, /<input\s+autoFocus\s+aria-label=\{tr\("新场景名称"/);
  assert.match(panel, /setSaving\(false\);\s*setTab\(value\)/);
});

test("long tile names use two aligned lines and preserve full hover labels", () => {
  assert.match(
    css,
    /\.ambience-tile \.ambience-tile-name\s*\{[^}]*-webkit-line-clamp: 2;[^}]*min-height: 2\.8em/,
  );
  assert.match(css, /\.ambience-tile > span > svg\s*\{[^}]*flex-shrink: 0/);
  assert.match(panel, /title=\{asset.name\}/);
  assert.match(panel, /title=\{item.name\}/);
  assert.match(panel, /title=\{pet.displayName/);
});
