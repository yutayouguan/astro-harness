import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const [panel, styles] = await Promise.all(
  [
    "../../components/settings/ToolsPanel.tsx",
    "../../styles/features/tools.css",
  ].map((path) => readFile(new URL(path, import.meta.url), "utf8")),
);

function rule(css, selector) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return css.match(new RegExp(`${escaped}\\s*\\{(?<body>[\\s\\S]*?)\\}`))
    ?.groups?.body;
}

test("tool gallery grid owns no outer glass surface", () => {
  const body = rule(styles, ".agent-tools-body");
  assert.ok(body, "missing tools body rule");
  assert.match(body, /border:\s*0;/);
  assert.match(body, /background:\s*transparent;/);
  assert.match(body, /box-shadow:\s*none;/);
  assert.match(body, /backdrop-filter:\s*none;/);
});

test("gallery cards expose summaries and open the existing detail view", () => {
  assert.match(panel, /className="tool-card-body agent-tool-detail"/);
  assert.match(
    panel,
    /setSelectedDetailId\(tool\.id\);[\s\S]*?setViewMode\("detail"\);/,
  );
  assert.match(panel, /className="agent-tool-param-summary"/);
  assert.match(panel, /className="agent-tool-open-hint"/);
  assert.match(panel, /callCount > 0/);

  const cardBody = rule(
    styles,
    ".tool-card.agent-tool-card .tool-card-body.agent-tool-detail",
  );
  assert.ok(cardBody, "missing gallery card body rule");
  assert.match(cardBody, /border-top:/);
  assert.match(cardBody, /background:\s*transparent;/);
  assert.match(cardBody, /box-shadow:\s*none;/);
  assert.match(styles, /-webkit-line-clamp:\s*2;/);
});

test("full schemas remain in the materialized detail panel", () => {
  assert.match(panel, /className="agent-tool-params-list is-detail"/);
  assert.match(panel, /className="agent-tool-param is-detail"/);

  const detail = rule(styles, ".tools-detail-panel");
  assert.ok(detail, "missing detail panel rule");
  assert.match(detail, /background:\s*var\(--settings-panel-background/);
  assert.match(detail, /backdrop-filter:\s*var\(--settings-panel-backdrop/);

  const detailBody = rule(styles, ".tools-detail-panel .tools-detail-body");
  assert.ok(detailBody, "missing flat detail body rule");
  assert.match(detailBody, /background:\s*transparent;/);
  assert.match(detailBody, /border:\s*0;/);
});

test("approval settings split the structural grid into independent surfaces", () => {
  const approvals = rule(styles, ".approvals-section");
  assert.ok(approvals, "missing approvals layout rule");
  assert.match(approvals, /grid-template-columns:\s*repeat\(2,/);
  assert.match(approvals, /width:\s*min\(100%, 920px\);/);
  assert.match(approvals, /margin:\s*0 auto;/);
  assert.match(approvals, /border:\s*0;/);
  assert.match(approvals, /background:\s*transparent;/);
  assert.match(approvals, /box-shadow:\s*none;/);

  const panels = rule(styles, ".approvals-section > .tools-detail-section");
  assert.ok(panels, "missing independent approval panel rule");
  assert.match(panels, /border:\s*var\(--settings-panel-border-width/);
  assert.match(panels, /background:\s*var\(--settings-panel-background/);
  assert.match(panels, /box-shadow:\s*var\(\s*--settings-panel-shadow/);

  assert.match(panel, /approvals-mode-card/);
  assert.equal(panel.match(/approvals-scope-card/g)?.length, 2);
  assert.match(panel, /approvals-allowlist-card/);
});
