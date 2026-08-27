import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const panelUrl = new URL(
  "../../components/settings/SkillsPanel.tsx",
  import.meta.url,
);
const coreCssUrl = new URL(
  "../../styles/features/skills/core.css",
  import.meta.url,
);
const tabsCssUrl = new URL("../../styles/components/tabs.css", import.meta.url);
const mcpSectionUrl = new URL(
  "../../components/settings/McpSection.tsx",
  import.meta.url,
);
const mcpToolsUrl = new URL(
  "../../hooks/providers/useMcpTools.ts",
  import.meta.url,
);
const messagesUrl = new URL("../../i18n/messages.ts", import.meta.url);

const panel = await readFile(panelUrl, "utf8");
const styles = await readFile(coreCssUrl, "utf8");
const tabs = await readFile(tabsCssUrl, "utf8");
const mcpSection = await readFile(mcpSectionUrl, "utf8");
const mcpTools = await readFile(mcpToolsUrl, "utf8");
const messages = await readFile(messagesUrl, "utf8");

test("plugin navigation keeps type and scope in one primary command row", () => {
  const commandBar = panel.indexOf('className="plugins-command-bar"');
  const primaryRow = panel.indexOf(
    'className="plugins-command-row plugins-command-row--primary"',
  );
  const typeTabs = panel.indexOf('className="skills-main-tabs"', primaryRow);
  const scopeTabs = panel.indexOf('className="plugins-scope-tabs"', primaryRow);
  const contextRow = panel.indexOf(
    'className="skills-toolbar plugins-context-toolbar"',
    primaryRow,
  );

  assert.ok(commandBar >= 0, "missing unified plugin command bar");
  assert.ok(primaryRow > commandBar, "missing primary command row");
  assert.ok(
    typeTabs > primaryRow,
    "plugin type tabs must live in the primary row",
  );
  assert.ok(scopeTabs > typeTabs, "scope tabs should follow plugin type tabs");
  assert.ok(
    contextRow > scopeTabs,
    "context controls should follow primary navigation",
  );
});

test("personal Skill sources share the contextual toolbar", () => {
  const contextRow = panel.indexOf(
    'className="skills-toolbar plugins-context-toolbar"',
  );
  const sourceTabs = panel.indexOf(
    'className="plugins-personal-tabs"',
    contextRow,
  );
  const controls = panel.indexOf('className="skills-toolbar-end"', sourceTabs);

  assert.ok(
    sourceTabs > contextRow,
    "source navigation must live in the context row",
  );
  assert.ok(
    controls > sourceTabs,
    "source navigation and controls must share one row",
  );
  assert.match(panel, /\["installed", "online", "machine"\]/);
  assert.match(panel, /item === "machine" \? "is-import"/);
  assert.doesNotMatch(panel, /className="plugins-primary-row"/);
  assert.doesNotMatch(
    panel,
    /className="skills-toolbar plugins-scope-toolbar"/,
  );
});

test("Skill and MCP navigation expose different source taxonomies", () => {
  assert.match(panel, /\["global", "builtin", "project"\]/);
  assert.match(panel, /\["personal", "public"\]/);
  assert.match(panel, /mcpScope === "public" \? "builtin" : "global"/);
  assert.match(messages, /"plugins\.scope\.project": "当前项目"/);
  assert.match(messages, /"plugins\.mcpScope\.public": "公开"/);
});

test("public MCP directory has category metadata and a scrollable filter rail", () => {
  for (const category of [
    "featured",
    "productivity",
    "development",
    "finance",
    "travel",
    "health",
    "research",
    "education",
    "communication",
    "analytics",
    "other",
  ]) {
    assert.match(mcpTools, new RegExp(`"${category}"`));
  }
  assert.match(panel, /MCP_PUBLIC_CATEGORY_IDS\.map/);
  assert.match(mcpSection, /server\.featured === true/);
  assert.match(mcpSection, /server\.category === publicCategory/);
  assert.match(
    tabs,
    /\.plugins-public-categories\s*\{[\s\S]*?overflow-x:\s*auto;/,
  );
});

test("plugin toolbar is compact and degrades to stacked rows on narrow screens", () => {
  assert.match(styles, /\.plugins-command-bar\s*\{[\s\S]*?gap:\s*8px;/);
  assert.match(
    tabs,
    /\.skills-page \.plugins-personal-tabs\s*\{[\s\S]*?background:\s*transparent;/,
  );
  assert.match(styles, /@media \(max-width:\s*860px\)/);
  assert.match(tabs, /@media \(max-width:\s*430px\)/);
});
