import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import test from "node:test";

const stylesRoot = new URL("../../styles/", import.meta.url);
const tokenFile = "tokens/component/glass.css";

function cssFiles(dir = "") {
  return readdirSync(new URL(dir || ".", stylesRoot), {
    withFileTypes: true,
  }).flatMap((entry) => {
    const path = `${dir}${dir ? "/" : ""}${entry.name}`;
    if (entry.isDirectory()) return cssFiles(path);
    return entry.name.endsWith(".css") ? [path] : [];
  });
}

function rules(source) {
  return [...source.matchAll(/([^{}]+)\{([^{}]*)\}/g)];
}

test("glass blur tiers live in tokens, not inline recipes", () => {
  const inline = [];
  for (const file of cssFiles()) {
    if (file === tokenFile) continue;
    const source = readFileSync(new URL(file, stylesRoot), "utf8");
    for (const [, selector] of source.matchAll(
      /blur\(\s*calc\(\s*[0-9.]+px \* var\(--glass-blur-scale/g,
    )) {
      inline.push(`${file}: ${selector}`);
    }
  }
  assert.deepEqual(
    inline,
    [],
    "模糊半径统一引用 --glass-blur-* 档位，避免同一配方散落各处",
  );
});

test("every backdrop-filter keeps its -webkit- twin", () => {
  const unpaired = [];
  for (const file of cssFiles()) {
    const source = readFileSync(new URL(file, stylesRoot), "utf8");
    for (const [, selector, body] of rules(source)) {
      if (!/(?<!-)\bbackdrop-filter\s*:/.test(body)) continue;
      if (/-webkit-backdrop-filter\s*:/.test(body)) continue;
      unpaired.push(`${file}: ${selector.trim().slice(0, 60)}`);
    }
  }
  assert.deepEqual(unpaired, [], "WKWebView 需要 -webkit-backdrop-filter 双写");
});
