import assert from "node:assert/strict";
import test from "node:test";
import { readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import ts from "typescript";

const read = (path) => readFile(new URL(path, import.meta.url), "utf8");
const source = await read("../../components/icons/AIActionIcon.tsx");
const css = await read("../../styles/components/ai-action-icon.css");
const compiled = ts.transpileModule(source, {
  compilerOptions: { jsx: ts.JsxEmit.ReactJSX, module: ts.ModuleKind.CommonJS },
}).outputText;
const module = { exports: {} };
new Function("require", "module", "exports", compiled)(
  createRequire(import.meta.url),
  module,
  module.exports,
);
const { AIActionIcon } = module.exports;

test("multiple AI icons have unique gradient IDs and valid local paint references", () => {
  const html = renderToStaticMarkup(
    createElement(
      "div",
      null,
      createElement(AIActionIcon),
      createElement(AIActionIcon, { variant: "search" }),
      createElement(AIActionIcon, { framed: true }),
    ),
  );
  const ids = [...html.matchAll(/<linearGradient id="([^"]+)"/g)].map(
    (m) => m[1],
  );
  assert.equal(ids.length, 3);
  assert.equal(new Set(ids).size, 3);
  for (const match of html.matchAll(/url\(#([^)]+)\)/g))
    assert.ok(ids.includes(match[1]));
  assert.equal((html.match(/aria-hidden="true"/g) ?? []).length, 3);
  assert.equal((html.match(/class="ai-icon-plate"/g) ?? []).length, 1);
});

test("icon size, rounded strokes and distinct search silhouette remain stable", () => {
  const create = renderToStaticMarkup(
    createElement(AIActionIcon, { size: 13 }),
  );
  const search = renderToStaticMarkup(
    createElement(AIActionIcon, { size: 24, variant: "search" }),
  );
  assert.match(create, /width="13" height="13"/);
  assert.match(search, /width="24" height="24"/);
  assert.match(create, /stroke-linecap="round"/);
  assert.equal((create.match(/<circle /g) ?? []).length, 4);
  assert.notEqual(create, search);
  assert.doesNotMatch(create + search, /<image|<script|https?:/);
});

test("light and dark palettes plus monochrome accessibility fallback are explicit", () => {
  assert.match(css, /html\[data-theme="dark"\] \.ai-action-icon/);
  for (const stop of ["cyan", "blue", "purple", "pink"])
    assert.match(css, new RegExp(`stop-color: var\\(--ai-icon-${stop}\\)`));
  assert.match(css, /prefers-contrast: more/);
  assert.match(css, /forced-colors: active/);
  assert.match(css, /stroke: currentColor/);
  assert.match(css, /fill: currentColor/);
  assert.match(css, /\.ai-action-icon \.ai-icon-fill\s*\{\s*stroke: none;/);
  assert.doesNotMatch(css, /animation:|!important/);
});

test("AI entry points adopt shared icons without replacing busy indicators or ordinary search", async () => {
  for (const file of [
    "settings/WallpaperSettingsCard",
    "settings/PetCreatePanel",
    "settings/PetSceneLibrary",
    "loop/LoopEditor",
    "loop/configs/ConfigField",
    "loop/LoopAiAssistant",
    "settings/EvolutionModelsPanel",
    "settings/MemoryPanel",
    "settings/CompressionSettingsCard",
  ]) {
    assert.match(await read(`../../components/${file}.tsx`), /<AIActionIcon/);
  }
  assert.match(
    await read("../../components/settings/PetCreatePanel.tsx"),
    /<Loader2 className="desktop-pet-spinner"/,
  );
  assert.match(
    await read("../../components/loop/configs/ConfigField.tsx"),
    /<Loader2 size=\{13\} className="loop-spin"/,
  );
  assert.doesNotMatch(
    await read("../../components/ui/ExpandableSearch.tsx"),
    /AIActionIcon/,
  );
});

test("selected AI launchers keep their surface on hover and disabled buttons lose elevation", async () => {
  assert.match(
    css,
    /:hover:not\(:disabled, \.is-active, \[aria-pressed="true"\]\)/,
  );
  assert.match(
    css,
    /\.ai-action-button:is\(\.is-active, \[aria-pressed="true"\]\)/,
  );
  assert.match(
    css,
    /\.ai-action-button:disabled\s*\{[^}]*opacity: 0\.45;[^}]*box-shadow: none;/,
  );
  assert.match(
    await read("../../components/loop/LoopEditor.tsx"),
    /aria-pressed=\{showAiAssistant\}/,
  );
  assert.match(
    await read("../../components/loop/configs/ConfigField.tsx"),
    /aria-busy=\{loading\}/,
  );
});
