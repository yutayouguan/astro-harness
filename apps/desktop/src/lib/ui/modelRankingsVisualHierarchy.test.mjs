import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { readI18nCatalogs } from "./i18nCatalogSource.mjs";

const [panelSource, iconSource, marketCss, rankingsCss, messagesSource] =
  await Promise.all([
    readFile(
      new URL(
        "../../components/settings/ModelRankingsPanel.tsx",
        import.meta.url,
      ),
      "utf8",
    ),
    readFile(
      new URL("../../components/icons/ProviderIcons.tsx", import.meta.url),
      "utf8",
    ),
    readFile(
      new URL("../../styles/features/model-market.css", import.meta.url),
      "utf8",
    ),
    readFile(
      new URL("../../styles/features/model-rankings.css", import.meta.url),
      "utf8",
    ),
    readI18nCatalogs(),
  ]);

test("task rankings expose composition and redundant change direction", () => {
  assert.match(panelSource, /className="mm-task-composition"/);
  assert.match(panelSource, /--mm-task-category-share/);
  assert.match(panelSource, /TASK_CATEGORY_COLORS\[id\.toLowerCase\(\)\]/);
  assert.match(panelSource, /className="mm-rank-change-arrow"/);
  assert.match(panelSource, /item\.change >= 0 \? "↑" : "↓"/);
  assert.match(panelSource, /aria-pressed=\{activeTask\?\.id === task\.id\}/);
  assert.match(panelSource, /function taskIconFor\(/);
  assert.match(panelSource, /className="mm-task-item-icon"/);
});

test("ranking hierarchy keeps surfaces neutral and comparison marks legible", () => {
  assert.match(rankingsCss, /\.mm-task-composition\s*\{/);
  assert.match(rankingsCss, /--mm-task-category-color/);
  assert.match(rankingsCss, /\.mm-rank-bar\s*\{[\s\S]*?height:\s*5px/);
  assert.match(rankingsCss, /\.mm-task-list button\.active::before/);
  assert.match(
    rankingsCss,
    /grid-template-columns:\s*24px minmax\(0, 1fr\) auto/,
  );
  assert.match(rankingsCss, /\.mm-task-item-icon\s*\{/);
  assert.match(
    rankingsCss,
    /grid-template-columns:\s*20px 22px minmax\(0, 1fr\) auto auto/,
  );
});

test("ranking navigation is one aligned rounded toolbar", () => {
  assert.match(
    rankingsCss,
    /\.mm-rankings-sticky-nav\s*\{[\s\S]*?border-radius:\s*14px/,
  );
  assert.match(
    rankingsCss,
    /\.mm-rankings-commandbar\s*\{[\s\S]*?align-items:\s*center/,
  );
  assert.match(
    rankingsCss,
    /\.mm-rankings-commandbar \.model-market-refresh\s*\{[\s\S]*?height:\s*34px/,
  );
  assert.match(marketCss, /--model-market-switcher-background:/);
  assert.match(
    rankingsCss,
    /--model-intel-switcher-background:\s*var\(\s*--model-market-switcher-background/,
  );
  assert.match(
    rankingsCss,
    /\.model-market > \.mm-rankings-workspace\s*\{[\s\S]*?padding-top:\s*0/,
  );
  assert.match(
    panelSource,
    /className="mm-rankings-sticky-nav"[\s\S]*?className="mm-rankings-commandbar"[\s\S]*?<\/div>\s*<\/div>\s*\{section === "usage"/,
  );
  assert.match(
    rankingsCss,
    /\.mm-rank-modality-tabs button\s*\{[\s\S]*?border-radius:\s*999px/,
  );
});

test("intelligence cards share the catalog card material", () => {
  assert.match(marketCss, /--model-market-card-background:/);
  assert.match(
    marketCss,
    /\.model-market-card\s*\{[\s\S]*?background:\s*var\(--model-market-card-background\)/,
  );
  assert.match(
    rankingsCss,
    /--model-intel-card-background:\s*var\(\s*--model-market-card-background/,
  );
  assert.match(
    rankingsCss,
    /\.mm-rank-primary-grid\s*\{[\s\S]*?background:\s*var\(--model-intel-card-background\)/,
  );
  assert.match(
    rankingsCss,
    /\.mm-task-layout,[\s\S]*?\.mm-app-rankings\s*\{[\s\S]*?background:\s*var\(--model-intel-card-background\)/,
  );
  assert.doesNotMatch(rankingsCss, /background:\s*rgba\(13, 10, 24, 0\.74\)/);
});

test("ranked model icons expose brand identity and attribution is localized", () => {
  assert.match(iconSource, /data-brand=\{brand\}/);
  for (const brand of [
    "anthropic",
    "openai",
    "google",
    "deepseek",
    "kimi",
    "zhipu",
  ]) {
    assert.match(rankingsCss, new RegExp(`data-brand="${brand}"`));
  }
  assert.match(messagesSource, /"modelRankings\.attribution": "数据来源：/);
  assert.match(messagesSource, /"modelRankings\.attribution": "Source:/);
});

test("ranking tabs auto-load each request key only once per panel mount", () => {
  assert.match(panelSource, /useRef\(new Set<string>\(\)\)/);
  assert.match(panelSource, /autoLoadedKeys\.current\.add\(key\)/);
  assert.match(
    panelSource,
    /shouldAutoLoadRankings\(envelope, autoLoadedKeys\.current\.has\(key\)\)/,
  );
  assert.doesNotMatch(panelSource, /attemptedAt/);
});

test("visible app rankings use contiguous display positions", () => {
  assert.match(
    panelSource,
    /apps\.slice\(0, 20\)\.map\(\(app, index\)[\s\S]*?className="mm-rank-position">\{index \+ 1\}<\/span>/,
  );
  assert.doesNotMatch(panelSource, /\{app\.rank \|\| index \+ 1\}/);
});

test("every visible ranked app links to its validated official website", () => {
  assert.match(panelSource, /className="mm-app-official-link"/);
  assert.match(panelSource, /href=\{app\.websiteUrl\}/);
  assert.match(panelSource, /target="_blank"/);
  assert.match(panelSource, /rel="noreferrer noopener"/);
  assert.match(rankingsCss, /\.mm-app-official-link:focus-visible/);
});
