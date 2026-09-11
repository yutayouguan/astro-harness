import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const desktopRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");

export function firstLayerDeclaration(css) {
  const match = css
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .match(/@layer\s+([^;{]+)([;{])/);
  return match
    ? {
        names: match[1].split(",").map((name) => name.trim()),
        statement: match[2] === ";",
      }
    : null;
}

export function assertLayerOrder(css, expected) {
  assert.deepEqual(
    firstLayerDeclaration(css),
    { names: expected, statement: true },
    "The first built CSS layer declaration must establish the complete global order. A component layer loaded first can override UI typography with base font: inherit.",
  );
}

export function builtStyleHrefs(html) {
  return [...html.matchAll(/<link\b[^>]*>/gi)].flatMap(([tag]) => {
    if (!/\brel\s*=\s*["']stylesheet["']/i.test(tag)) return [];
    const href = tag.match(/\bhref\s*=\s*["']([^"']+)["']/i)?.[1];
    return href ? [href] : [];
  });
}

export function checkBuiltStyleLayers(dist = resolve(desktopRoot, "dist")) {
  const expected = firstLayerDeclaration(
    readFileSync(resolve(desktopRoot, "src/styles/index.css"), "utf8"),
  );
  assert.ok(
    expected?.statement,
    "The style entry must declare the canonical layer order",
  );
  const hrefs = builtStyleHrefs(
    readFileSync(resolve(dist, "index.html"), "utf8"),
  );
  assert.ok(hrefs.length, "No entry CSS found in the production HTML");
  const css = hrefs
    .map((href) => {
      assert.ok(!/^(?:[a-z]+:)?\/\//i.test(href), "Entry CSS must be local");
      const path = resolve(dist, href.replace(/^\/+/, "").split(/[?#]/)[0]);
      assert.ok(path.startsWith(resolve(dist) + sep), "Entry CSS escaped dist");
      return readFileSync(path, "utf8");
    })
    .join("\n");
  assertLayerOrder(css, expected.names);
  return hrefs;
}

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(resolve(process.argv[1])).href
) {
  const hrefs = checkBuiltStyleLayers(
    process.argv[2] ? resolve(process.argv[2]) : undefined,
  );
  console.log(
    `Production CSS layer order verified (${hrefs.length} stylesheet${hrefs.length === 1 ? "" : "s"}).`,
  );
}
