import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const [tabs, segmented, index, market, rankings] = await Promise.all(
  [
    "../../styles/components/tabs.css",
    "../../styles/tokens/component/segmented.css",
    "../../styles/index.css",
    "../../components/settings/ModelMarketPanel.tsx",
    "../../components/settings/ModelRankingsPanel.tsx",
  ].map((path) => readFile(new URL(path, import.meta.url), "utf8")),
);

test("every semantic settings tab uses the model-service material contract", () => {
  assert.match(segmented, /\.settings-content-inline \[role="tablist"\]/);
  assert.match(segmented, /\.settings-content-inline \[role="tab"\]/);
  assert.match(
    tabs,
    /\.settings-content-inline \[role="tablist"\]\s*\{[\s\S]*?border:\s*1px solid var\(--seg-shell-border\);[\s\S]*?background:\s*var\(--seg-shell-bg\);[\s\S]*?box-shadow:\s*var\(--seg-shell-shadow\);[\s\S]*?backdrop-filter:\s*var\(--seg-shell-blur\);/,
  );
  assert.match(
    tabs,
    /\[role="tab"\]:is\(\[aria-selected="true"\], \.active, \.is-active\)[\s\S]*?background:\s*var\(--seg-item-bg-active\);[\s\S]*?box-shadow:\s*var\(--seg-item-shadow-active\);/,
  );
});

test("model market catalog and intelligence navigation are semantic tabs", () => {
  assert.match(market, /className="model-market-surface-tabs" role="tablist"/);
  assert.equal(market.match(/role="tab"/g)?.length, 2);
  assert.match(rankings, /className="mm-rankings-sections" role="tablist"/);
  assert.match(rankings, /className="mm-rank-modality-tabs" role="tablist"/);
  assert.match(rankings, /className="mm-rank-inline-tabs"/);
});

test("the shared tab override loads after settings feature styles", () => {
  const marketStyles = index.indexOf("features/model-market.css");
  const rankingStyles = index.indexOf("features/model-rankings.css");
  const sharedTabs = index.indexOf("components/tabs.css");

  assert.ok(marketStyles >= 0);
  assert.ok(rankingStyles > marketStyles);
  assert.ok(sharedTabs > rankingStyles);
});
