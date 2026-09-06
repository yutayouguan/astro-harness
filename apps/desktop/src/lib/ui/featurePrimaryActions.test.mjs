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

test("workflow creation actions form the left toolbar group without a visible title", () => {
  assert.match(
    loopPanel,
    /className="loop-toolbar-start"[\s\S]*?loop-create-group[\s\S]*?loop\.create[\s\S]*?loop\.templateTitle/,
  );
  assert.doesNotMatch(loopPanel, /className="loop-toolbar-title"/);
  assert.match(loopStyles, /\.loop-toolbar-start\s*\{[\s\S]*?display:\s*flex;/);
  assert.match(
    loopStyles,
    /\.loop-toolbar-end\s*\{[\s\S]*?margin-left:\s*auto;/,
  );
});

test("workflow template mode replaces the saved workflow content", () => {
  assert.match(
    loopPanel,
    /\{showTemplates && \([\s\S]*?className="loop-template-picker"/,
  );
  assert.match(
    loopPanel,
    /className="loop-content" hidden=\{showTemplates\}[\s\S]*?renderGallery\(\)/,
  );
  assert.match(
    loopStyles,
    /\.loop-template-picker\s*\{[\s\S]*?border-radius:\s*16px;[\s\S]*?background:/,
  );
});

test("scheduled task creation actions form the left toolbar group without a visible title", () => {
  assert.match(
    cronPanel,
    /className="cron-toolbar-start"[\s\S]*?cron-create-group[\s\S]*?cron\.create[\s\S]*?cron\.createFromTemplate/,
  );
  assert.doesNotMatch(cronPanel, /className="cron-toolbar-title"/);
  assert.match(cronPanel, /aria-expanded=\{showTemplates\}/);
  assert.match(cronPanel, /className="cron-template-picker"/);
  assert.match(cronStyles, /\.cron-toolbar-start\s*\{[\s\S]*?display:\s*flex;/);
  assert.match(
    cronStyles,
    /\.cron-toolbar-end\s*\{[\s\S]*?margin-left:\s*auto;/,
  );
});
