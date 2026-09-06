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
const segmentedCss = readFileSync(
  new URL("../../styles/tokens/component/segmented.css", import.meta.url),
  "utf8",
);

function rule(css, selector) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return css.match(new RegExp(`${escaped}\\s*\\{(?<body>[\\s\\S]*?)\\n\\}`))
    ?.groups?.body;
}

test("model market filters share the segmented active material", () => {
  const market = rule(marketCss, ".model-market");
  assert.ok(market, "missing model market surface");
  assert.doesNotMatch(market, /--tone(?:-soft)?:/);
  assert.match(segmentedCss, /\.model-market,/);

  const activeRules = [
    [
      rankingsCss,
      ".model-market-surface-tabs button.active,\n.mm-rankings-sections button.active,\n.mm-rank-modality-tabs button.active,\n.mm-rank-inline-tabs button.active",
    ],
    [marketCss, ".model-market-sort-btn.active"],
    [marketCss, ".model-market-view-btn.active"],
    [marketCss, ".model-market-type-btn.active"],
    [marketCss, ".model-market-filter-btn.active"],
  ];

  for (const [css, selector] of activeRules) {
    const active = rule(css, selector);
    assert.ok(active, `missing active recipe for ${selector}`);
    assert.match(active, /var\(--seg-item-color-active\)/);
    assert.match(active, /var\(--seg-item-bg-active\)/);
    assert.match(active, /var\(--seg-item-shadow-active\)/);
  }

  assert.doesNotMatch(
    marketCss,
    /html\[data-theme="dark"\] \.model-market-(?:sort|view|filter)-btn\.active/,
  );

  for (const selector of [
    ".model-market-sort-btn:hover",
    ".model-market-view-btn:hover",
    ".model-market-type-btn:hover",
    ".model-market-filter-btn:hover",
  ]) {
    const hover = rule(marketCss, selector);
    assert.ok(hover, `missing hover recipe for ${selector}`);
    assert.match(hover, /var\(--seg-item-color-hover\)/);
    assert.match(hover, /var\(--seg-item-bg-hover\)/);
  }
});
