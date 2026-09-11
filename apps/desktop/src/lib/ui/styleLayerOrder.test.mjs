import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import {
  assertLayerOrder,
  builtStyleHrefs,
  firstLayerDeclaration,
} from "../../../scripts/check-style-layers.mjs";

const order = [
  "vendor",
  "tokens.primitive",
  "tokens.semantic",
  "tokens.component",
  "base",
  "shell",
  "components",
  "features",
  "overrides",
];
const prelude = `@layer ${order.join(",")};`;
const read = (path) => readFileSync(new URL(path, import.meta.url), "utf8");

test("the production check catches a feature layer created before the global order", () => {
  assert.throws(() =>
    assertLayerOrder(`@layer features {.panel{padding:1px}}${prelude}`, order),
  );
  assert.doesNotThrow(() =>
    assertLayerOrder(`${prelude}@layer features {.panel{padding:1px}}`, order),
  );
  assert.equal(
    firstLayerDeclaration("/* @layer fake; */ body{margin:0}"),
    null,
  );
});
test("global styles load before any component side-effect imports", () => {
  const main = read("../../main.tsx")
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/\/\/[^\n]*/g, "");
  assert.match(main.trimStart(), /^import "\.\/styles\/index\.css";/);
  assert.deepEqual(firstLayerDeclaration(read("../../styles/index.css")), {
    names: order,
    statement: true,
  });
});
test("ambience uses the global feature layer rather than creating one during component evaluation", () => {
  assert.doesNotMatch(
    read("../../components/ui/DesktopAmbienceButton.tsx"),
    /import\s+["'][^"']*desktop-ambience\.css/,
  );
  assert.match(
    read("../../styles/index.css"),
    /@import "\.\/features\/desktop-ambience\.css" layer\(features\);/,
  );
  assert.doesNotMatch(
    read("../../styles/features/desktop-ambience.css"),
    /@layer/,
  );
});
test("production entry CSS is read in HTML link order, without modulepreloads", () => {
  assert.deepEqual(
    builtStyleHrefs(
      '<link rel="modulepreload" href="a.js"><link crossorigin href="/assets/main.css" rel="stylesheet"><link rel=\'stylesheet\' href=\'./b.css\'>',
    ),
    ["/assets/main.css", "./b.css"],
  );
});
