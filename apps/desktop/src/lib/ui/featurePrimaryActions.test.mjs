import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const loopPanel = await readFile(
  new URL("../../components/loop/LoopPanel.tsx", import.meta.url),
  "utf8",
);
const cronPanel = await readFile(
  new URL("../../components/schedule/CronPanel.tsx", import.meta.url),
  "utf8",
);
const loopStyles = await readFile(
  new URL("../../styles/features/loop/panel.css", import.meta.url),
  "utf8",
);
const cronStyles = await readFile(
  new URL("../../styles/features/cron/base.css", import.meta.url),
  "utf8",
);

test("workflow title and creation actions form the left toolbar group", () => {
  assert.match(
    loopPanel,
    /className="loop-toolbar-start"[\s\S]*?page\.loop\.title[\s\S]*?loop-create-group[\s\S]*?loop\.create[\s\S]*?loop\.templateTitle/,
  );
  assert.match(loopStyles, /\.loop-toolbar-start\s*\{[\s\S]*?display:\s*flex;/);
  assert.match(loopStyles, /\.loop-toolbar-end\s*\{[\s\S]*?margin-left:\s*auto;/);
});

test("scheduled task title and creation actions form the left toolbar group", () => {
  assert.match(
    cronPanel,
    /className="cron-toolbar-start"[\s\S]*?page\.cron\.title[\s\S]*?cron-create-group[\s\S]*?cron\.create[\s\S]*?cron\.createFromTemplate/,
  );
  assert.match(cronPanel, /aria-expanded=\{showTemplates\}/);
  assert.match(cronPanel, /className="cron-template-picker"/);
  assert.match(cronStyles, /\.cron-toolbar-start\s*\{[\s\S]*?display:\s*flex;/);
  assert.match(cronStyles, /\.cron-toolbar-end\s*\{[\s\S]*?margin-left:\s*auto;/);
});
