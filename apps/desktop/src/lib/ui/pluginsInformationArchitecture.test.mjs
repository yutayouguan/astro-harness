import assert from "node:assert/strict";
import { access, readFile } from "node:fs/promises";
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
const toolsCssUrl = new URL("../../styles/features/tools.css", import.meta.url);
const mcpSectionUrl = new URL(
  "../../components/settings/McpSection.tsx",
  import.meta.url,
);
const mcpToolsUrl = new URL(
  "../../hooks/providers/useMcpTools.ts",
  import.meta.url,
);
const messagesUrl = new URL("../../i18n/messages.ts", import.meta.url);
const publicMcpCatalogUrl = new URL(
  "../../config/mcp-public-catalog.json",
  import.meta.url,
);
const publicMcpCatalogLoaderUrl = new URL(
  "../../config/mcpPublicCatalog.ts",
  import.meta.url,
);
const communityMcpCatalogUrl = new URL(
  "../../config/mcp-community-catalog.json",
  import.meta.url,
);
const mcpCatalogSourcesUrl = new URL(
  "../../config/mcp-catalog-sources.json",
  import.meta.url,
);
const mcpBrandIconUrl = new URL(
  "../../components/icons/McpBrandIcon.tsx",
  import.meta.url,
);

const panel = await readFile(panelUrl, "utf8");
const styles = await readFile(coreCssUrl, "utf8");
const tabs = await readFile(tabsCssUrl, "utf8");
const toolsCss = await readFile(toolsCssUrl, "utf8");
const mcpSection = await readFile(mcpSectionUrl, "utf8");
const mcpTools = await readFile(mcpToolsUrl, "utf8");
const messages = await readFile(messagesUrl, "utf8");
const publicMcpCatalog = JSON.parse(
  await readFile(publicMcpCatalogUrl, "utf8"),
);
const publicMcpCatalogLoader = await readFile(
  publicMcpCatalogLoaderUrl,
  "utf8",
);
const communityMcpCatalog = JSON.parse(
  await readFile(communityMcpCatalogUrl, "utf8"),
);
const mcpCatalogSources = JSON.parse(
  await readFile(mcpCatalogSourcesUrl, "utf8"),
);
const mcpBrandIcon = await readFile(mcpBrandIconUrl, "utf8");

test("plugin catalog remounts after hook-signature edits during Fast Refresh", () => {
  assert.match(
    panel,
    /^\/\/ @refresh reset/m,
    "the state-heavy plugin catalog must not reuse a stale hook queue after edits",
  );
});

test("plugin primary row keeps type, scope, and controls together", () => {
  const commandBar = panel.indexOf('className="plugins-command-bar"');
  const primaryRow = panel.indexOf(
    'className="plugins-command-row plugins-command-row--primary"',
  );
  const typeTabs = panel.indexOf('className="skills-main-tabs"', primaryRow);
  const scopeTabs = panel.indexOf('className="plugins-scope-tabs"', primaryRow);
  const controls = panel.indexOf('className="skills-toolbar-end"', scopeTabs);

  assert.ok(commandBar >= 0, "missing unified plugin command bar");
  assert.ok(primaryRow > commandBar, "missing primary command row");
  assert.ok(
    typeTabs > primaryRow,
    "plugin type tabs must live in the primary row",
  );
  assert.ok(scopeTabs > typeTabs, "scope tabs should follow plugin type tabs");
  assert.ok(
    controls > scopeTabs,
    "sort, search, view, and refresh controls should stay in the primary row",
  );
});

test("personal Skill sources use a second underline row", () => {
  const primaryRow = panel.indexOf(
    'className="plugins-command-row plugins-command-row--primary"',
  );
  const controls = panel.indexOf('className="skills-toolbar-end"', primaryRow);
  const sourceTabs = panel.indexOf(
    'className="plugins-personal-tabs plugins-personal-tabs--underline"',
    controls,
  );

  assert.ok(
    sourceTabs > controls,
    "personal source navigation should follow the primary command row",
  );
  assert.match(panel, /\["installed", "online", "machine", "updates"\]/);
  assert.match(panel, /plugins-personal-tab--underline/);
  assert.doesNotMatch(panel, /className="plugins-primary-row"/);
  assert.doesNotMatch(
    panel,
    /className="skills-toolbar plugins-scope-toolbar"/,
  );
  assert.doesNotMatch(panel, /className="skills-toolbar plugins-context-toolbar"/);
});

test("Skill and MCP navigation expose different source taxonomies", () => {
  assert.match(panel, /\["global", "builtin", "project"\]/);
  assert.match(panel, /\["personal", "public"\]/);
  assert.match(panel, /mcpScope === "public" \? "builtin" : "global"/);
  assert.match(messages, /"plugins\.scope\.project": "当前项目"/);
  assert.match(messages, /"plugins\.mcpScope\.public": "公开"/);
});

test("public MCP directory exposes the catalog taxonomy and aggregate filters", () => {
  for (const category of [
    "featured",
    "all",
    "official",
    "remote",
    "development",
    "productivity",
    "database",
    "search",
    "web-scraping",
    "file-system",
    "version-control",
    "communication",
    "cloud-service",
    "cloud-storage",
    "marketing",
    "finance",
    "design",
    "memory",
    "other",
  ]) {
    assert.match(mcpTools, new RegExp(`"${category}"`));
  }
  assert.match(panel, /MCP_PUBLIC_CATEGORY_IDS\.map/);
  for (const label of [
    "精选",
    "全部",
    "官方 🌟",
    "远程 MCP",
    "数据库",
    "网页抓取",
    "文件系统",
    "版本控制",
    "云服务",
    "云存储",
    "营销",
    "设计",
    "记忆",
  ]) {
    assert.match(messages, new RegExp(`: "${label}"`));
  }
  assert.match(mcpSection, /server\.featured === true/);
  assert.match(mcpSection, /publicCategory === "all"/);
  assert.match(mcpSection, /publicCategory === "official"/);
  assert.match(mcpSection, /server\.catalogOfficial === true/);
  assert.match(mcpSection, /publicCategory === "remote"/);
  assert.match(mcpSection, /server\.catalogRemote === true/);
  assert.match(mcpSection, /server\.category === publicCategory/);
  assert.match(
    tabs,
    /\.plugins-public-categories\s*\{[\s\S]*?flex-wrap:\s*wrap;/,
  );
  assert.match(
    tabs,
    /@media \(max-width: 430px\)[\s\S]*?\.plugins-public-categories\s*\{[\s\S]*?overflow-x:\s*auto;/,
  );
});

test("online Skills expose sort tabs, flattened scenes, and API Key filter", () => {
  assert.doesNotMatch(panel, /className="skills-store-source-filter"/);
  assert.match(panel, /className="skills-store-sort-tabs"/);
  assert.match(panel, /className="skills-store-category-tags"/);
  assert.match(panel, /skills-store-category-tag/);
  assert.match(panel, /className="skills-store-api-key-filter"/);
  assert.equal(panel.match(/selectionIndicator="radio"/g)?.length, 1);
  assert.match(panel, /STORE_CATEGORY_IDS\.map/);
  assert.match(styles, /\.skills-store-category-tags\s*\{[\s\S]*?flex-wrap:\s*wrap;/);
  assert.match(panel, /"all"[\s\S]*"trending"[\s\S]*"downloads"[\s\S]*"recent"/);
});

test("public MCP directory is config-driven and uses catalog entry categories", () => {
  assert.equal(publicMcpCatalog.version, 1);
  assert.ok(publicMcpCatalog.servers.length >= 20);
  assert.equal(
    new Set(publicMcpCatalog.servers.map((server) => server.id)).size,
    publicMcpCatalog.servers.length,
    "public MCP ids must be unique",
  );

  const entryCategories = new Set([
    "development",
    "productivity",
    "database",
    "search",
    "web-scraping",
    "file-system",
    "version-control",
    "communication",
    "cloud-service",
    "cloud-storage",
    "marketing",
    "finance",
    "design",
    "memory",
    "other",
  ]);

  for (const server of publicMcpCatalog.servers) {
    assert.ok(server.name && server.description);
    assert.ok(entryCategories.has(server.category), `${server.id} has an unknown category`);
    assert.ok(server.type === "stdio" || server.type === "streamableHttp");
    assert.ok(
      server.type === "stdio" ? server.command : server.url,
      `${server.id} must provide a usable transport configuration`,
    );
    assert.equal(server.env, undefined, `${server.id} must not embed secret values`);
    assert.equal(server.headers, undefined, `${server.id} must not embed secret headers`);
  }

  assert.match(publicMcpCatalogLoader, /mcp-public-catalog\.json/);
  assert.match(mcpSection, /PUBLIC_MCP_CATALOG/);
  assert.match(mcpSection, /installableMcpServer/);
  assert.match(mcpSection, /mcpTools\.install/);
});

test("public MCP brands accept bundled assets and HTTPS catalog icons with a safe fallback", async () => {
  const brandedServers = publicMcpCatalog.servers.filter((server) => server.icon);
  assert.ok(brandedServers.length >= 20, "most public MCP entries should have brand icons");

  for (const server of brandedServers) {
    assert.match(server.icon, /^[a-z0-9-]+$/, `${server.id} has an unsafe icon id`);
    await access(new URL(`../../../public/mcp-icons/${server.icon}.svg`, import.meta.url));
  }

  assert.match(mcpSection, /<McpBrandIcon icon=\{server\.icon\}/);
  assert.match(
    mcpBrandIcon,
    /src=\{validIcon \? `\/mcp-icons\/\$\{validIcon\}\.svg` : remoteIcon\}/,
  );
  assert.match(mcpBrandIcon, /const HTTPS_ICON_URL = \/\^https:/);
  assert.match(mcpBrandIcon, /className="mcp-brand-icon-placeholder"/);
  assert.match(mcpBrandIcon, /loading=\{remoteIcon \? "lazy" : "eager"\}/);
  assert.match(mcpBrandIcon, /referrerPolicy="no-referrer"/);
  assert.match(toolsCss, /\.mcp-brand-icon\.is-loaded/);
  assert.match(mcpBrandIcon, /return <McpIcon/);
});

test("MCP discovery merges the official Registry with categorized SSR snapshots", () => {
  assert.ok(communityMcpCatalog.servers.length > 1_000);
  const registryServers = communityMcpCatalog.servers.filter(
    (server) => server.catalogRegistered === true,
  );
  const remoteServers = communityMcpCatalog.servers.filter(
    (server) => server.catalogRemote === true,
  );
  assert.ok(registryServers.length > 400);
  assert.ok(remoteServers.length > 250);
  for (const server of communityMcpCatalog.servers) {
    assert.equal(server.catalogInstallable, false);
    assert.ok(
      server.catalogSource
        .split(" + ")
        .every((source) =>
          source === "mcpservers.org" || source === "registry.modelcontextprotocol.io"
        ),
    );
    assert.match(server.catalogSourceUrl, /^https:\/\//);
    assert.equal(server.command, undefined);
    assert.equal(server.url, undefined);
  }

  const registrySource = mcpCatalogSources.sources.find(
    (source) => source.kind === "registry-api",
  );
  const directorySource = mcpCatalogSources.sources.find(
    (source) => source.kind === "ssr-html",
  );
  assert.equal(registrySource?.path, "/v0/servers");
  assert.equal(registrySource?.query.version, "latest");
  assert.equal(directorySource?.search.pathTemplate, "/{locale}/search");
  assert.equal(directorySource?.search.queryParam, "query");
  for (const feed of directorySource.feeds) {
    assert.doesNotMatch(feed.path ?? feed.pathTemplate, /\/api(?:\/|$)/);
  }

  assert.match(publicMcpCatalogLoader, /mcp-community-catalog\.json/);
  assert.match(publicMcpCatalogLoader, /Discovery-only MCP entries cannot be installed/);
  assert.match(mcpSection, /server\.catalogInstallable === false/);
  assert.match(mcpSection, /server\.catalogRegistered/);
  assert.match(mcpSection, /server\.catalogRemote/);
  assert.match(mcpSection, /selectedServer\.catalogInstallable !== false/);
});

test("plugin toolbar is compact and degrades to stacked rows on narrow screens", () => {
  assert.match(styles, /\.plugins-command-bar\s*\{[\s\S]*?gap:\s*8px;/);
  assert.match(
    styles,
    /\.plugins-command-bar\s*\{[\s\S]*?padding:\s*0;/,
    "plugin command bar should rely on the shared feature-page inset",
  );
  assert.match(
    tabs,
    /\.skills-page \.plugins-personal-tabs\s*\{[\s\S]*?background:\s*transparent;/,
  );
  assert.match(styles, /@media \(max-width:\s*860px\)/);
  assert.match(tabs, /@media \(max-width:\s*430px\)/);
});
