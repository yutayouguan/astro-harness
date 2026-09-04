import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const [panelSource, iconSource, rankingsCss, messagesSource] =
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
      new URL("../../styles/features/model-rankings.css", import.meta.url),
      "utf8",
    ),
    readFile(new URL("../../i18n/messages.ts", import.meta.url), "utf8"),
  ]);

test("task rankings expose composition and redundant change direction", () => {
  assert.match(panelSource, /className="mm-task-composition"/);
  assert.match(panelSource, /--mm-task-category-share/);
  assert.match(panelSource, /TASK_CATEGORY_COLORS\[id\.toLowerCase\(\)\]/);
  assert.match(panelSource, /className="mm-rank-change-arrow"/);
  assert.match(panelSource, /item\.change >= 0 \? "↑" : "↓"/);
  assert.match(panelSource, /aria-pressed=\{activeTask\?\.id === task\.id\}/);
});

test("ranking hierarchy keeps surfaces neutral and comparison marks legible", () => {
  assert.match(rankingsCss, /\.mm-task-composition\s*\{/);
  assert.match(rankingsCss, /--mm-task-category-color/);
  assert.match(rankingsCss, /\.mm-rank-bar\s*\{[\s\S]*?height:\s*5px/);
  assert.match(rankingsCss, /\.mm-task-list button\.active::before/);
  assert.match(
    rankingsCss,
    /grid-template-columns:\s*20px 22px minmax\(0, 1fr\) auto auto/,
  );
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
