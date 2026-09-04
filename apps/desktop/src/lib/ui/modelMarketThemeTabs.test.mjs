import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const marketCss = readFileSync(
  new URL("../../styles/features/model-market.css", import.meta.url),
  "utf8",
);
const rankingsCss = readFileSync(
  new URL("../../styles/features/model-rankings.css", import.meta.url),
  "utf8",
);

function rule(css, selector) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return css.match(new RegExp(`${escaped}\\s*\\{(?<body>[\\s\\S]*?)\\n\\}`))
    ?.groups?.body;
}

test("model market tabs inherit the active document tone", () => {
  const market = rule(marketCss, ".model-market");
  assert.ok(market, "missing model market surface");
  assert.doesNotMatch(market, /--tone(?:-soft)?:/);

  const surfaceTab = rule(
    rankingsCss,
    ".model-market-surface-tabs button.active,\n.mm-rankings-sections button.active,\n.mm-rank-modality-tabs button.active,\n.mm-rank-inline-tabs button.active",
  );
  assert.ok(surfaceTab, "missing model market active-tab recipe");
  assert.match(surfaceTab, /var\(--tone\)/);

  const typeTab = rule(marketCss, ".model-market-type-btn.active");
  assert.ok(typeTab, "missing model type active-tab recipe");
  assert.match(typeTab, /var\(--tone\)/);
});
